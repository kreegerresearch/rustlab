# rustlab — Agent Reference

This file is the authoritative guide for AI coding tools working on this codebase.
Read it before making any changes.

---

## Project Overview

**rustlab** is a Rust CLI and scripting toolkit for matrix algebra and digital signal processing (DSP).
It provides a simple scripting language (`.rlab` files), an interactive REPL, and direct CLI commands for filter design, convolution, and plotting.

Key properties:
- All numeric types are complex by default (`Complex<f64>`)
- Scripting language uses 1-based indexing, `:` range syntax, and suppression with `;`
- Terminal plotting via `ratatui` + `crossterm` (braille-pixel charts, alternate screen)
- Five-crate Cargo workspace with strict no-cycle dependency order

---

## Repository Layout

```
rustlab/
├── Cargo.toml              # workspace root — shared deps, resolver = "2"
├── AGENTS.md               # this file
├── PLAN.md                 # original architecture plan
├── README.md               # user-facing documentation
├── llms.txt                # AI reference — pointers to docs files
├── crates/
│   ├── rustlab-core/       # primitive types and traits — no DSP, no plotting
│   ├── rustlab-dsp/        # DSP algorithms — depends on core only
│   ├── rustlab-plot/       # ratatui charts + HTML export — depends on core only
│   ├── rustlab-proto/      # IPC wire protocol for rustlab↔viewer communication
│   ├── rustlab-viewer/     # standalone egui plot viewer (separate binary)
│   ├── rustlab-script/     # .rlab language interpreter — depends on core, dsp, plot
│   └── rustlab-cli/        # binary `rustlab` — depends on all crates
├── dev/
│   └── plans/              # multi-phase development plans (see section below)
├── perf/                   # performance benchmarks and reports
├── examples/               # 75 scripts organised by toolbox (one subdir per toolbox):
│   ├── language/           # syntax, lambdas, structs, save/load, TOML, profiling (11)
│   ├── math/               # complex basics, trig/special, random, ML activations (4)
│   ├── linalg/             # eig, matrix_ops, tensor3, rank, roots, linear_algebra (6)
│   ├── stats/              # stats aggregates, all/any (2)
│   ├── sparse/             # sparse solve, complex sparse LU, eigs, sparse vec/mat (4)
│   ├── dsp/                # FIR/IIR, Kaiser, firpm, upfirdn, fixed-point (6)
│   ├── spectral/           # fft, pwelch, spectrogram, cwt (4)
│   ├── controls/           # transfer functions, bode, nyquist, root locus, PID, LQR (15)
│   ├── rf/                 # S-parameters, Smith chart, cascade, amplifier stability (6)
│   ├── pde/                # laplacian / BCs, electrostatics, dielectric, vector_calc (5)
│   ├── plot/               # contour, quiver, surf, heatmap, masks, animation (8)
│   ├── audio/              # real-time PCM: filter, spectrum monitor, platform launchers (4)
│   │   ├── filter.rlab          # FIR lowpass script used by all launchers
│   │   ├── passthrough.rlab     # minimal stdin→stdout loopback
│   │   ├── spectrum_monitor.rlab  # live two-panel terminal plot (waveform + FFT)
│   │   ├── spectrum_monitor.sh # platform-aware launcher (macOS/Linux/synthetic)
│   │   ├── macos.sh          # sox-based live audio pipeline (macOS)
│   │   ├── linux.sh          # arecord/aplay pipeline (Linux ALSA)
│   │   ├── wsl.sh            # PulseAudio / WSL2 pipeline
│   │   ├── tcp.sh            # socat/nc TCP streaming (cross-platform)
│   │   └── test_filter.sh    # CI-friendly end-to-end test (no mic/speakers)
│   └── notebooks/          # source `.md` notebooks rendered by `rustlab-notebook`
└── docs/
    ├── examples.md         # annotated walkthroughs of each example script
    ├── functions.md        # full function reference with signatures and examples
    └── quickref.md         # concise capability index kept in sync with actual builtins
```

**Dependency order (no cycles):**
```
rustlab-core
    ↑           ↑
rustlab-dsp   rustlab-plot ←(optional viewer feature)── rustlab-proto
    ↑           ↑                                            ↑
    └─────┬─────┘                                    rustlab-viewer
    rustlab-script                                   (separate binary)
          ↑
    rustlab-cli
```

---

## Performance Analysis

### Running benchmarks

```sh
make perf
```

This builds the release binary, times every script in `perf/`, measures
binary size, and writes `perf/report.md`.

### When an AI agent sees `perf/report.md`

If `perf/report.md` exists and was generated in the current session (or the
user asks for a performance review), the agent **must**:

1. Read `perf/report.md` in full.
2. Work through every task listed in the `<!-- AI_ANALYSIS -->` block inside
   that file.
3. Append a `## AI Analysis` section directly to `perf/report.md` with
   findings and a prioritised recommendation table.
4. If any **open** optimisation (OPT-1, OPT-2, OPT-3, …) can be applied
   without breaking tests, implement it, re-run `make perf`, and update the
   report with before/after numbers.

Do not skip the analysis or produce it only as a chat response — it must be
written into the report file so it is preserved across sessions.

---

## Development Plans

Multi-phase feature plans live in `dev/plans/`.  Each plan tracks current phase
and per-item status.  Completed plans are moved to `dev/plans/closed/` so the
top-level directory only shows what is still in flight; same convention for
`dev/requests/closed/`.  **Follow this protocol at the start and end of every
session that touches a plan:**

### Reading plans at session start

1. Check `dev/plans/` for any plan whose **Status** is not `complete`.
2. Read the active plan, identify the **Current phase** and which items in it
   are `not started` vs `in progress` vs `done`.
3. If the user has not already given direction, briefly surface the active plan:
   > "The controls plan is on **Phase 1** (Language Foundations).
   >  Would you like to continue with Phase 1, or work on something else?"

### Implementing a phase

- Work through every item in the phase top-to-bottom.
- After each item, mark it `done` in the plan file.
- On completion of the full phase:
  1. Update the plan: set the phase **Status** to `complete` and advance
     **Current phase** to the next phase.
  2. Run `cargo test --workspace` and confirm it passes.
  3. Ask the user: *"Phase N is complete.  Ready to start Phase N+1
     ([short description])?"*  Do not begin the next phase without an
     explicit yes.

### Plan file conventions

Each phase block contains a `**Status:**` line.  Valid values:
- `not started` — work has not begun
- `in progress` — partially implemented
- `complete` — all items done, tests pass

Update the top-level **Current phase** line and the per-phase **Status** line
together whenever a phase finishes.

---

## Active Plans

| Plan | File | Status |
|------|------|--------|
| Control Systems Toolbox | `dev/plans/closed/controls.md` | Complete — all 6 phases |
| Controls Bootcamp Functions | `dev/plans/closed/controls_bootcamp.md` | Complete — logspace, rk4, lyap, gram, care, dare, place, freqresp, svd |
| Lambda / Anonymous Functions | `dev/plans/closed/lambda.md` | Complete — both phases (lambdas, arrayfun, feval) |
| Function Call Profiling | `dev/plans/closed/profiling.md` | Complete — both phases (profile(), --profile flag) |
| Real-Time Audio Streaming | `dev/plans/closed/audio_streaming.md` | Complete — all 3 phases (while loop, FirState, audio I/O) |
| Live Plot & Spectrum Monitor | `dev/plans/closed/live_plot.md` | Complete — all 3 phases (LiveFigure, builtins, mag2db) |
| Sparse Vectors and Matrices | `dev/plans/closed/sparse.md` | Complete — all 4 phases (types, conversion, arithmetic, solver/utilities) |
| Notebook System | `dev/plans/closed/notebook_report.md` | Complete through Phase 6 (parse, execute, KaTeX, LaTeX/PDF, polish, multi-notebook) + light/dark theme support |
| Notebook Future Features | `dev/plans/notebook_future.md` | Complete — template interpolation, string arrays, categorical bar charts, categorical line plots (`plot(labels, y)`) |
| Notebook Mermaid Diagrams | `dev/plans/closed/notebook_mermaid.md` | Phase 1 complete — pure-Rust SVG via `mermaid-rs-renderer` behind the `mermaid` Cargo feature on `rustlab-notebook` (default-on for the standalone bin). The main `rustlab` CLI no longer ships a `notebook` subcommand at all, so the historical `notebook-mermaid` feature flag is gone. Inline `<svg>` in HTML, `\includesvg` figure in LaTeX/PDF, ` ```mermaid ` fence in Markdown. Hashed cache, `<!-- caption: -->`/`<!-- hide -->`/`<!-- details: -->` directives. |
| Notebook File Embeds (transclusion) | `dev/plans/closed/notebook_file_embeds.md` | Complete — Obsidian-style `![[Document]]`, `![[Document#Heading]]`, `![[Document#^block-id]]` transclusion. Pre-process pass before `parse_notebook`; embedded ` ```rustlab ` blocks share the host evaluator. Heading demotion per nesting level (cap h6), recursion cap = 4, cycle detection. Errors render as inline `[!CAUTION]` callouts. Block-id `^id` markers stripped from rendered output of every notebook. See `examples/notebooks/_setup.md` + `embeds_demo.md` and `docs/notebooks.md` § "File embeds (transclusion)". |
| Notebook Obsidian Vault Integration | `dev/plans/closed/notebook_obsidian_vault.md` | Complete — both phases shipped. Phase A: `--obsidian` (markdown format) does five vault-native rewrites: cross-notebook links → `[[wikilinks]]`, plots → `_attachments/<stem>/`, frontmatter merge (`tags:[rustlab]` / `cssclasses:[rustlab-notebook]`), trailing iframe (suppress with `--no-iframe`), auto-generated `index.md` in directory mode. CLI flags `--attachments-dir <DIR>`, `--no-iframe`. Phase B: `rustlab-notebook watch <dir>` long-running re-renderer using `notify` with debounced events (default 250 ms), embed dependency graph for precise re-renders, post-render reference-driven plot-dir sweep, failure isolation. Markdown-only currently. See `docs/notebooks.md` § "Obsidian integration" and "Live preview with `notebook watch`". |
| Notebook math passes through verbatim | `crates/rustlab-notebook/src/lib.rs` (`Format::Markdown` branch) | Complete — `--format markdown` writes math spans (`$…$`, `$$…$$`) byte-identical to the notebook source. GitHub's KaTeX path does not apply CommonMark backslash-escape or emphasis-pairing inside math spans, so the natural LaTeX (`\,`, `\;`, `\:`, `\!`, `\|`, `^*`, `_*`) reaches the renderer unmodified. Earlier alpha-synonym (`\thinspace` …) and double-backslash (`\\,` …) rewrites both rendered as literal text on github.com (see `rustlab_llm/AGENTS.md` § "Renderer regression (rustlab 0.3.1)") and have been removed; the deleted `markdown_safe.rs` is in git history if context is needed. See `docs/notebooks.md` § "Markdown (`--format markdown`)". |
| Hand-Rolled Sparse Solver | `dev/plans/closed/sparse_solve_handroll.md` | Complete — all 5 phases (CSC, sparse Cholesky for SPD, sparse LU with partial pivoting, basic AMD ordering, builtin dispatch). Full Davis-AMD with external degree deferred. |
| `rustlab_em` Feature Requests (the §2026-04 sweep) | `dev/plans/em_requests_plan.md` + `dev/plans/em_requests_queue.md` | In progress — Items 1, 2, 3 shipped (masks, sparse solve, Laplacian variants); Item 4 next (`eigs`); Items 5-7 pending. Source request: `../rustlab_em/dev/rustlab/requests/em_requests.md`. |
| Original `rustlab_em` Requests (5 originals) | `dev/plans/closed/rustlab_em_requests.md` | Complete — all five EM requests landed (vector calc, quiver/streamplot, contour, laplacian_2d, animation export). |
| `rustlab_llm` Gap Closure (v0.3) | `~/.claude/plans/lively-roaming-abelson.md` | Complete — all four open gaps shipped: short-circuit `&&`/`\|\|`, `M(I)` linear-index gather (with `M(scalar)` flip), `layernorm(M)` matrix overload, multi-output user functions `function [a, b] = name(x)`. Tour example/notebook in `examples/language_v0_3.{rlab,md}`. |
| Notebook / Plot Follow-ups (2026-05-16) | `dev/plans/notebook_followups_2026_05_16.md` | Mostly shipped — B1 (cwd guard), B2 (post-render plot sweep), B3 (write_output hash cache), P2 (`self_writes` u64 digest), L1 (`rustlab-notebook check` linter, 7 checks) all done. P1 (snapshot memory cap) and P3 (`strip_render_artifacts` single-pass) deferred until profiling justifies. Each item is self-contained — file paths, fix sketch, and tests listed in the plan doc. |
| Obsidian Plugin Phase 1 — `rustlab-notebook render --format json` | `dev/plans/notebook_obsidian_plugin.md` § Phase 1 | Shipped — JSON emitter in `rustlab-notebook/src/render_json.rs` plus `cmd_render_json` in `lib.rs`. CLI flags `--format json`, `--stdin`, `--cwd <DIR>`, `--pretty`. Schema version 1: prose blocks ship with pre-rendered HTML, code blocks with inline SVG plots + `blake3:` source hash, mermaid with inline SVG (under the `mermaid` feature), callouts with HTML, exercise/solution markers. Output goes to stdout only — JSON has no `--output` path. Lives entirely in `rustlab-notebook` per the "keep `rustlab` binary small" rule (main `rustlab` CLI does not depend on this crate). Phase-1 scope: SVG plots only; Plotly HTML and animation GIFs reserved as future `JsonPlot` formats. Phases 2–4 (the actual Obsidian TypeScript plugin) are out-of-tree, separate work. |
| RF S-Parameter / Smith Chart Toolbox | `dev/plans/closed/sparameters.md` | Complete — all 6 phases shipped 2026-05-17. Touchstone v1.1 + v2-keyword-tolerant reader, optional noise-parameter block, conversions (S↔Z↔Y↔T↔ABCD, mixed-mode), cascade, deembed, newref, interp_freq, Smith chart, rfplot 2×2 review, VSWR/return loss/insertion loss, Rollett K + µ-parameters, simultaneous-conjugate-match Γms/Γml, MAG/MSG, stability and gain circles, smith_circle overlay, s2td time-domain via IFFT. 50+ builtins under one S-Parameters (RF) help category. Tests: 1185 in rustlab-script (analytic anchors include matched attenuator round-trips, isolator stability K=∞, series-resistor cascade additivity, mixed-mode mode-isolation for symmetric thru pair, simultaneous-conjugate-match defining identity Γin(Γml) = conj(Γms)). Outstanding follow-ups noted in the closed plan: Touchstone writer doesn't round-trip noise data; per-port `[Reference]` Z0 lists rejected (workaround: use single-ended export + `s2smm`); Octave-comparison harness deferred (stock Octave has no `sparameters` equivalent). |

---

## Workflow Rules

These three rules apply to every task, no exceptions.

### 1. Plan first, implement second

Before writing any code for a non-trivial change, produce a written plan and present it to the user for review. The plan must cover:
- What will change and why
- Which files and crates are affected
- Any trade-offs or risks
- The test strategy for the new code

Do not begin implementation until the plan is explicitly approved.

### 2. Tests are required for new features

Every new DSP algorithm, builtin function, or scripting language feature must ship with at least one meaningful unit test. "Meaningful" means:
- It exercises a concrete, verifiable property (e.g. lowpass coefficients sum to 1, convolution with a delta is identity, `inv(A) * A ≈ I`)
- It would catch a regression if the implementation were broken
- It runs headlessly without a TTY (`cargo test --workspace`)

Add tests in the same PR/commit as the feature — never defer them. Good locations:
- `crates/rustlab-dsp/src/tests.rs` — DSP algorithms
- `crates/rustlab-script/src/tests.rs` — interpreter and builtins (use `run()` to evaluate snippets)
- `crates/rustlab-cli/tests/examples.rs` — integration / example scripts

### 3. Every new feature ships with docs and REPL help

Any commit that adds or changes a builtin function, scripting construct, or CLI feature **must** include all three of the following in the same commit — not as a follow-up:

1. **`docs/functions.md`** — add or update the function's section with its full signature, description, and at least one usage example.
2. **REPL `HelpEntry`** — add a `HelpEntry { name, brief, detail }` record in `crates/rustlab-cli/src/commands/repl.rs`.
3. **Toolbox assignment** — add the function name to exactly one `CategoryRow` in `CATEGORIES` (same file). The 12 toolboxes are `language`, `math`, `linalg`, `stats`, `sparse`, `dsp`, `spectral`, `controls`, `rf`, `pde`, `plot`, `audio` (see `TOOLBOXES` for display order). Pick the existing subcategory that fits, or add a new `CategoryRow` with a new subcategory string. The `help_coverage_tests` unit tests fail the build if a `HELP` entry has no toolbox or appears in more than one.

A feature is not done until a user can type `help foo` in the REPL and get a useful answer. Do not treat documentation as optional cleanup.

### 4. Never commit or push without explicit approval

Do not run `git commit` or `git push` automatically, even when work is complete and all tests pass. Present a summary of what changed and wait for the user to explicitly say to commit and/or push.

### 5. Keep `docs/functions.md` current

`docs/functions.md` is the canonical scripting reference. It must be updated in the same commit as any change that affects it:

- **New builtin function** — add its signature, description, and example to the appropriate section.
- **New Value type** — document its fields and how to use it.
- **New language construct** — add syntax and example to the Language section.
- **New toolbox feature** (controls, DSP, etc.) — add it to the relevant toolbox section.

`llms.txt` at the repo root is a short pointer to the four main docs files (`docs/quickref.md`, `docs/functions.md`, `docs/examples.md`, `README.md`); it does not need content updates. Do not treat docs updates as optional cleanup.

### 6. Keep `docs/quickref.md` current

`docs/quickref.md` is the concise capability index used by AI agents to discover what rustlab can do. It must stay in sync with the actual registered builtins. Update it in the same commit as any change that affects it:

- **New builtin function** — add it to the appropriate section (Math, Statistics, DSP, etc.).
- **New language construct** — add it to the Language table.
- **New category** (e.g. a new toolbox) — add a new section.
- **Removed or renamed function** — remove or rename the entry immediately; stale entries mislead other agents.

Do not list functions that are not implemented. `quickref.md` must reflect reality, not intentions.

**Periodic accuracy check:** At the start of any session that touches builtins or language features, quickly verify that `quickref.md` still matches `r.register(...)` calls in `crates/rustlab-script/src/eval/builtins.rs`. If entries are stale or missing, fix them in the same commit.

### 7. Update `AGENTS.md` after every new feature

After implementing any new feature, update `AGENTS.md` in the same commit:

- **New builtin function** — add it to the "All builtin functions" table in the Scripting Language Reference section.
- **New language construct** — add it to the Grammar or Key language behaviours table.
- **New crate or module** — add it to Repository Layout and the relevant Crate Details section.
- **New workflow rule or convention** — add it to the appropriate section (Workflow Rules, Error Handling, Design Decisions).
- **New CLI subcommand** — add it to the `rustlab-cli` Crate Details section.
- **New Common Task pattern** — add a how-to entry under Common Tasks.

`AGENTS.md` is the agent's primary orientation document. Keeping it current means the next session starts with accurate context instead of having to re-discover what changed.

### 8. Never commit secrets or sensitive information

Before staging any file, check that it does not contain:
- SSH private keys (any `-----BEGIN ... PRIVATE KEY-----` block)
- API keys, tokens, or bearer credentials
- Passwords or passphrases
- `.env` files or any file whose name matches `.env*`
- AWS/GCP/Azure credentials or config files with embedded secrets

If a file that may contain secrets is found in the working tree, warn the user immediately and do not stage or commit it under any circumstances. Use `.gitignore` to prevent accidental staging. This rule cannot be overridden by any user instruction.

### 9. Plotting changes must verify all output backends

Any change to a plot type, axis behaviour, label rendering, or
coordinate convention **must** be validated across every backend
that renders that plot type, not just the one you're working in.
A bug fix in `file.rs` (SVG) that leaves `html.rs` (Plotly),
`viewer_live.rs` (live viewer), or `ascii.rs` (terminal) in a
divergent state is a regression, not a fix.

**The backends to consider for any 2-D plot type:**

| Backend | Crate / file | Notes |
|---|---|---|
| Terminal (TUI) | `rustlab-plot::ascii` | `ratatui`. Renders to alternate screen on `Terminal` context; silently no-ops under `Notebook` / `Headless`. |
| SVG (notebook markdown, gallery) | `rustlab-plot::file` (`SVGBackend`) | Reached via `savefig("foo.svg")`, `rustlab-notebook render -f markdown`, gallery rebuilds. Shared with the LaTeX path. |
| PNG | `rustlab-plot::file` (`BitMapBackend`) | Reached via `savefig("foo.png")`. Same `render_to_backend` code as SVG, so SVG fixes usually inherit; verify per change anyway. |
| LaTeX / PDF | `rustlab-notebook::render_latex` + the SVG path | LaTeX `\includesvg`/`\includegraphics` the SVG plots, so PDF inherits SVG. PDF builds run `pdflatex`/`tectonic`; verify they still compile. |
| HTML (Plotly) | `rustlab-plot::html` | `savefig("foo.html")`, `rustlab-notebook render -f html`, the `--obsidian` iframe. Plotly's defaults frequently disagree with plotters'. |
| Live viewer | `rustlab-plot::viewer_live` + `rustlab-viewer::figure` | Wire protocol from the notebook process to the egui viewer. Texture/axis conventions are egui-specific and must be reconciled with the static-output backends. |
| Animation (GIF / HTML) | `rustlab-plot::animation` | GIF goes through `BitMapBackend` (inherits SVG). HTML animations use Plotly frames (inherits HTML). |

**What "validate" means:**

1. Add a unit test in each backend's test module that pins the
   convention (e.g. an SVG test that grep-parses cell positions, an
   HTML test that asserts the right Plotly trace attribute is
   emitted). The notebook `cross_backend` tests are a good template
   pattern.
2. Render the user-reproducer notebook through every backend the
   plot type targets and inspect the artefacts. The `rustlab-notebook
   render` driver handles HTML, LaTeX/PDF, and markdown/SVG in one
   pass; the viewer needs a separate manual smoke pass.
3. If a backend genuinely can't replicate the convention (e.g. the
   TUI has no axis labels and so can't be inconsistent about them),
   document that explicitly in the code comment.

**Why this is its own rule.** The 2026-05-16 `imagesc` bug was a SVG
fix shipped without checking HTML or the live viewer — the SVG read
right while the viewer kept rendering the original mismatch. Cross-
backend consistency for plotting is so easy to forget that it gets
its own rule.

### 10. Core functionality must be written in pure Rust

**Core** = functions, algorithms, numerics, DSP, linear algebra, anything a script-level builtin exposes as math. **Infrastructure** = graphics, plotting, terminal UI, file I/O, parsing, serialization, error formatting.

**Default rule:**

| Category | Default | Examples |
|---|---|---|
| Core (algorithms, math, DSP, numerics) | **Pure Rust, hand-rolled** | sparse solvers, FFT, filter design, Laplacian stencils, eigensolvers, special functions |
| Infrastructure | Library OK if license permits | `plotters`, `ratatui`, `ndarray`, `num-complex`, `serde`, `clap`, `toml` |

For core work, "we use a Rust crate for this" is *not* a sufficient reason on its own — even MIT-Apache pure-Rust crates count as imported algorithm code that escapes our review and our debugger. Build it ourselves unless there is a clear, written-down reason not to.

**Exception process — when a core-work library is genuinely the right call:**

If you believe a library buys enough advantage to justify pulling it in for core functionality, **do not silently add it.** Open a written trade-off study before any code lands:

```
## Trade-off study: <crate name> for <use case>

### What we'd hand-roll
- Rough LoC estimate, days of senior work, risk surface (numerical
  stability, edge cases, test burden).
- What the resulting in-tree code would look like and where it lives.

### What the crate gives us
- Specific feature(s) we need.
- Crate license, transitive dependency count, last release date, maintainer.
- Total compiled size impact (estimate via `cargo tree` and a build
  diff).
- API churn risk: how often have they bumped major versions?

### Pros of pulling it in
- Concrete time saved, concrete capability we can't reasonably build.

### Cons of pulling it in
- New dependency surface, supply-chain exposure, code we can't debug
  to the line, future migration cost.

### Recommendation
- Pull it in / hand-roll / hybrid (e.g. use crate for X, hand-roll Y).
- If recommending a pull-in, list the specific commit / version pinned.
```

File the study in `dev/plans/<topic>-tradeoff.md` and link it from the implementation PR. The user makes the call, not the agent.

**Hard limits that override even a good trade-off study:**
- No GPL / LGPL / AGPL / copyleft.
- No Fortran / C++ FFI. Pure-Rust crates only for core work.
- No "large library" — broadly, anything bringing >1 MB of compiled code or >10 transitive deps is suspect for core work and needs strong justification.
- No vendored numerical libraries that the curriculum is supposed to teach (e.g. don't import a sparse-solver crate for a curriculum that explicitly walks through how sparse solvers work).

**Why:** Core functionality is rustlab's *value proposition* — the curriculum is partly about students reading the algorithms running their physics. Vendored solvers undermine that. Infrastructure is plumbing — let mature crates handle it.

### 11. Non-trivial work ships on a feature branch via PR

`main` is a protected branch — `git push origin main` is rejected
server-side for everyone, including admins. Any new feature, new
subcommand, renderer change, or other multi-file / multi-commit work
must develop on a topic branch and land via a pull request.

**Trivial-exception list** (still OK direct to main):
- single-line typo fixes in prose,
- workspace version bumps,
- CI workflow tweaks (e.g. adding a missing apt dep, tightening path
  filters),
- `.gitignore` carve-outs.

Everything else — yes, even "just" a documentation update that
introduces a new convention — branches.

**Standard flow:**

```sh
git switch -c feature/<short-name>            # branch from main
# work, commit; reuse the usual "commit only on user approval" rule
git push -u origin feature/<short-name>
gh pr create --title "..." --body "..."       # open PR
gh pr merge --squash --delete-branch          # or merge via the web UI
```

`workflow_dispatch` runs of `validate-notebooks` from a feature branch
are encouraged before opening the PR — surface CI noise on the branch,
not on a freshly-merged main.

**Why:** the `rustlab-notebook validate` subcommand landed via 9
sequential commits directly on main in 2026-05-23, four of which had
red CI runs. A feature branch + PR would have kept main's history
clean, confined the CI noise to the branch, and given the work a
single PR-shaped review surface. Branch protection on `main` now
enforces this mechanically.

---

## Build & Test

```sh
# Build everything
cargo build --workspace

# Run tests
cargo test --workspace

# Generate API docs
cargo doc --workspace --no-deps --open

# Run a script directly without installing
cargo run -p rustlab-cli --bin rustlab -- run examples/dsp/lowpass.rlab
```

### Testing dependencies

`cargo test` is the only requirement for the standard test suite. Higher-tier
checks pull in external tools — install only what you need for the targets
you run.

| Tool | Required by | macOS | Ubuntu / Debian | Notes |
|------|-------------|-------|------------------|-------|
| `rustc`, `cargo` | everything | https://rustup.rs | https://rustup.rs | mandatory |
| `octave` | `make octave-compare` | `brew install octave` | `apt-get install octave` | release-mandatory |
| `pdflatex` (TeX Live) | PDF render in `make notebooks` / `rustlab-notebook validate … -f pdf` | `brew install --cask mactex-no-gui` | `apt-get install texlive-latex-recommended texlive-fonts-recommended texlive-latex-extra texlive-pictures` | needs TS1 fonts (`cm-super`) for full Unicode coverage |
| `tectonic` | PDF render alt | `brew install tectonic` | `cargo install tectonic` | used if `pdflatex` not in PATH |
| `inkscape` | PDF render — `svg.sty` shells out to it to convert plot SVGs | `brew install inkscape` | `apt-get install inkscape` | required whenever PDF is requested |
| `markdownlint-cli2` | `make validate-notebooks` (markdown) | `npm i -g markdownlint-cli2` | `npm i -g markdownlint-cli2` | optional |
| `vnu` (W3C Nu) | `make validate-notebooks` (html, preferred) | `brew install vnu` | https://validator.github.io/validator/ | optional; alternatively set `VNU_JAR=/path/to/vnu.jar` |
| `tidy-html5` (5.x+) | `make validate-notebooks` (html, fallback) | `brew install tidy-html5` | `apt-get install tidy` | optional; macOS's `/usr/bin/tidy` is the 2006 HTML4 build and is auto-skipped |
| `chktex` | `make validate-notebooks` (latex) | `brew install chktex` | `apt-get install chktex` | optional; standalone, does not pull in TeX Live |
| `pdfinfo`, `pdftotext` (poppler) | `make validate-notebooks` (pdf smoke) | `brew install poppler` | `apt-get install poppler-utils` | optional |
| `qpdf` | `make validate-notebooks` (pdf structure) | `brew install qpdf` | `apt-get install qpdf` | optional |
| `verapdf` | `rustlab-notebook validate` (pdf PDF/A, only with `--pdf-a`) | https://verapdf.org/software/ | https://verapdf.org/software/ | opt-in; pipeline does not target PDF/A by default |

`rustlab-notebook validate` shells out to each linter only when it's
present and reports `SKIPPED` + an install hint for anything missing.
Pass `--require-all` to upgrade any `SKIPPED` to a hard failure — the
CI workflow at `.github/workflows/validate-notebooks.yml` uses this to
enforce a baseline toolchain.

### Installing the binary

`make install` works on both macOS and Linux. It installs to `~/.local/bin` by default and detects the OS to run `codesign` only on macOS:

```sh
make install                          # → ~/.local/bin/rustlab
make install INSTALL_DIR=/usr/local/bin   # override destination
# or via cargo on any platform:
cargo install --path crates/rustlab-cli   # → ~/.cargo/bin/rustlab
```

> **macOS note:** Copying a binary with `cp` invalidates its ad-hoc code signature.
> `make install` handles this automatically. If you copy the binary manually, run:
> `codesign --sign - --force <destination>/rustlab`

> **Linux note:** Building `rustlab-plot` needs `libfontconfig1-dev`
> and `libfreetype-dev` on the system (`apt-get install
> libfontconfig1-dev libfreetype-dev pkg-config`). The `plotters/ttf`
> feature pulls in `font-kit` → `yeslogic-fontconfig-sys` /
> `freetype-sys`, both of which link against system libraries; the
> macOS SDK provides them implicitly.

### Octave numerical comparison

The repo ships a regression harness that cross-checks rustlab's math/DSP/linalg/ODE
output against GNU Octave to machine precision. Driver: `tests/octave/run_compare.sh`,
invoked via:

```sh
make octave-compare          # requires `octave` on PATH (brew install octave / apt-get install octave)
```

The target regenerates all `out*.csv` (rustlab) and `ref*.csv` (Octave) files in
`tests/octave/`, then runs both `compare.m` (19 DSP cases) and `compare_full.m`
(112+ cases covering math, stats, matrix, linalg, sparse Laplacians, sparse direct
solve, sparse eigs, geometry masks, vector calculus, real-typed elem-ops, FFT,
and a swath of edge cases — empty/single-element inputs, banker's rounding, NaN
boundaries, dynamic-range log, boundary-condition stencils, etc.). It exits
nonzero if any case exceeds its per-suite tolerance (`T_EXACT=1e-9`,
`T_FILTER=1e-6`, `T_FIRPM=1e-4`, `T_ITER=1e-4`). To add a new function, append
a `save(...)` line to `rustlab_full.rlab` and a matching `csvwrite(...)` +
`check(...)` pair to `reference_full.m` / `compare_full.m`.

**Note:** the harness uses `octave --no-gui --no-window-system` to avoid Qt
library issues on macOS Homebrew installs. If `--no-window-system` is missing,
upgrade Octave (`brew upgrade octave`).

---

## Pre-Release Procedure

Releases are cut from `main`. Every step below must pass before tagging.
Follow Workflow Rule 4: never commit, push, or tag without explicit user
approval.

### 1. Clean working tree

```sh
git status                                                # must be clean
git fetch origin && git log HEAD..origin/main --oneline   # must be empty
```

If the tree has unrelated in-progress work, pause and surface it to the user
before continuing.

### 2. Lint and format

```sh
cargo fmt --all -- --check                          # mandatory — must pass
cargo clippy --workspace --features viewer          # advisory — review output
```

`cargo fmt --check` must pass with no diff. `cargo clippy` must compile (no
hard clippy errors), but warnings are currently advisory — the workspace has
accumulated style/complexity warnings that will be addressed in a dedicated
cleanup pass. Review clippy output for anything in the `correctness` or
`suspicious` categories; those must be fixed before release even though the
overall gate is advisory. Do not silence warnings with `#[allow(...)]` just
to pass — fix the underlying issue, or add the attribute with a comment
explaining why it's the correct call (e.g. method name intentionally shadows
a std trait for API ergonomics).

Once the cleanup pass lands, tighten this step to `cargo clippy --workspace
--features viewer -- -D warnings`.

### 3. Build and test both feature configurations

```sh
cargo build --workspace                           # default features
cargo test  --workspace                           # default features
cargo build --workspace --features viewer         # with viewer
cargo test  --workspace --features viewer         # with viewer
```

All four must succeed. The viewer feature is off by default, so the no-viewer
build must stay green — if it breaks, users installing via `cargo install
--path crates/rustlab-cli` hit the failure first.

### 4. Run the performance benchmarks twice

```sh
make perf                       # first run
make perf                       # second run — rules out cold-cache noise
```

On **both** runs:

- All seven `bench_*.rlab` scripts must report `PASS`.
- All timings must be under the thresholds declared in the `AI_ANALYSIS` block
  of `perf/run_perf.sh` (`bench_builtins > 300 ms`, `bench_fft > 100 ms`, etc.).
  If a single run exceeds a threshold but the other is clean, investigate
  (thermal throttle, background load) before proceeding; do not raise the
  threshold to make a release pass.
- Numeric output must be sane: `convolve` length = `len(x)+len(h)-1`, FFT
  round-trip length matches input, scalar-loop sum Σ(1..10000) = 50005000,
  etc. A `PASS` status alone is not enough — the values must also be correct.

### 5. Run the Octave numerical comparison (mandatory)

```sh
make octave-compare             # requires `octave` on PATH
```

All cases in `compare.m` (19 DSP) and `compare_full.m` (67 math/linalg/DSP/ODE)
must pass their per-suite tolerances. Octave is a **hard requirement** for
release — if it is not installed, install it (`brew install octave` /
`apt-get install octave`) and re-run. Do not skip.

### 6. Notebook render smoke test

The notebook binary ships with every release; exercise it end-to-end so a
broken templating / KaTeX / figure-snapshot change does not slip out:

```sh
./target/release/rustlab-notebook render examples/notebooks/quick_look.md -o /tmp/nb.html
./target/release/rustlab-notebook render examples/notebooks/quick_look.md -o /tmp/nb.pdf --format pdf
./target/release/rustlab-notebook render examples/notebooks/quick_look.md -o /tmp/nb.md --format markdown
./target/release/rustlab-notebook render examples/notebooks/quick_look.md -o /tmp/nb.md --format markdown --obsidian
```

Open the artifacts and confirm code blocks, math, and plots render. The
`--obsidian` variant should additionally append an `<iframe>` to the
sibling `.html` at the bottom of the `.md`.

For an automatable version of step 6 — one that runs renders through
trusted external linters and reports a per-format PASS/FAIL — use the
`validate` subcommand:

```sh
rustlab-notebook validate examples/notebooks/                  # all 4 formats
rustlab-notebook validate examples/notebooks/ -f html,pdf      # restrict formats
rustlab-notebook validate examples/notebooks/ --require-all    # CI: missing linter → fail
rustlab-notebook validate examples/notebooks/ --report json    # machine-readable
rustlab-notebook validate examples/notebooks/ --pdf-a          # add verapdf PDF/A
rustlab-notebook validate examples/notebooks/ --keep-tmp       # inspect renders
rustlab-notebook validate examples/notebooks/ \
    --linter vnu=$HOME/jars/vnu.jar --linter chktex=/opt/bin/chktex
```

Wrapped by `make validate-notebooks` for convenience. The
implementation lives at `crates/rustlab-notebook/src/validate.rs`.
Linter selection:

| Format   | Linter (in order)                                                 |
|----------|-------------------------------------------------------------------|
| markdown | `markdownlint-cli2` → `markdownlint`                              |
| html     | `vnu` (via `$VNU_JAR` or `--linter vnu=…`) → `tidy-html5` (5.x+; macOS's 2006 HTML4 tidy is auto-skipped) |
| latex    | `chktex`                                                          |
| pdf      | `pdfinfo` + `pdftotext` (smoke) → `qpdf --check` (structure) → `verapdf` (PDF/A, opt-in via `--pdf-a`) |

Each linter shells out only when installed; otherwise the row reports
`SKIPPED` plus an install hint. Exit codes: `0` clean, `1` any FAIL,
`2` `--require-all` set and a linter is missing. This is output-side
validation; the complementary source-side linter is `rustlab-notebook
check`. For per-platform install commands for every linter (and the
PDF render toolchain — `pdflatex`, `inkscape`), see the *Testing
dependencies* table under [Build & Test](#build--test).

**Downstream projects.** Because `validate` ships in the
`rustlab-notebook` binary itself, a project that depends on rustlab
(e.g. `rustlab_em`) consumes it with no extra script vendoring:

```sh
cargo install rustlab-notebook                  # one-time per dev / CI host
rustlab-notebook validate notebooks/            # run from the downstream repo
rustlab-notebook validate notebooks/ --report json > validation.json   # CI
```

When a downstream notebook surfaces a renderer bug, file a rustlab
issue with the source `.md`, the `--report json` capture, and (for PDF
failures) the auto-preserved `<stem>.log` build log that
`compile_pdf` writes next to the output path.

**Self-test.** The linter wrappers are guarded by their own unit tests:

```sh
make validate-notebooks-self-test
# equivalent: cargo test -p rustlab-notebook --lib validate::tests
```

These exercise each wrapper against a deliberately-broken fixture
under `crates/rustlab-notebook/tests/fixtures/bad/` and assert the
wrapper reports FAIL — guarding against silent regressions where a
wrapper stops detecting real problems (wrong exit-code mapping, swapped
grep pattern, etc.).

**JSON report schema.** `--report json` emits a versioned document:

```json
{
  "schema_version": 1,
  "summary": { "pass": 33, "fail": 0, "skipped": 5 },
  "results": [
    { "fixture": "quick_look", "format": "pdf", "linter": "qpdf",
      "status": "OK", "detail": "" }
  ]
}
```

Pin against `schema_version` in downstream tooling; additive changes
keep the same version, breaking changes bump it.

**CI integration.** `.github/workflows/validate-notebooks.yml` runs
`cargo test -p rustlab-notebook --lib validate::tests` followed by
`rustlab-notebook validate examples/notebooks/ --format
html,markdown,latex --require-all` on every PR and main push that
touches the notebook crate, fixtures, or the workflow itself. PDF
validation is deliberately omitted from CI — installing pdflatex +
inkscape costs minutes per run for an artefact already exercised by
`make validate-notebooks` locally.

**Notebook source layout.** Sources live at `examples/notebooks/*.md`.
Generated files never mix with sources — `make notebooks` writes
everything into the top-level `gallery/` directory. The `.gitignore`
splits visibility:

- `gallery/<name>.md` and `gallery/plots/<name>/*.svg` are **committed**
  so GitHub displays the rendered notebooks inline. This is the primary
  entry point readers click into from the repo root README. When a
  notebook source changes, regenerate and commit the matching
  `gallery/<name>.md` plus any changed SVGs in the same commit, and
  update the row in `gallery/README.md` if the notebook's title or
  scope changed.
- `gallery/<name>.html` and `gallery/index.html` are **gitignored** —
  bulky Plotly bundles for local interactive browsing only.

See `examples/notebooks/README.md` for the directory layout and
`docs/notebooks.md` for the renderer design (incl. the
`plot_dir` / `plot_href_prefix` split shared by markdown and LaTeX).

**Notebook authoring rules for AI agents** — the canonical reference
lives in `docs/notebooks.md`. Highlights:

- **Math escaping**: `${expr}$` is math-wrap shorthand in plain text;
  bare `${expr}` inside an open `$...$` span emits the value without
  re-wrapping; `\$` is the literal-`$` escape for currency; in markdown
  tables, replace `|...|` cardinality with `\lvert ... \rvert` or the
  raw `|` will split the table cell on GitHub.
- **Callouts**: prefer GFM-native `> [!NOTE]` / `[!TIP]` /
  `[!IMPORTANT]` / `[!WARNING]` / `[!CAUTION]` blockquote syntax.
  Legacy `<!-- note -->` still parses and auto-migrates on render.
- **GFM superset**: footnotes (`[^id]`), task lists (`- [ ]` / `[x]`),
  explicit heading IDs (`{#anchor}`), and strikethrough are all on.
- **Wikilinks**: `[[Foo]]`, `[[Foo|alias]]`, `[[Foo#Section]]`,
  `![[image.png]]` parse in source; the renderer rewrites them to
  ordinary markdown links/images so the committed `book/*.md` is
  GitHub-safe and Obsidian-native.

See `dev/plans/closed/notebook_obsidian_alignment.md` for the design
rationale (which Obsidian features were adopted and which were skipped).

**PDF dependencies** (`--format pdf` only): `pdflatex` (or `tectonic`) plus
the LaTeX packages `svg`, `transparent`, `trimspaces`, `pagecolor`, and
Inkscape on PATH. rustlab invokes `pdflatex -shell-escape` so the `svg`
package can launch Inkscape to convert each plot SVG to PDF. Install on
macOS with:

```sh
brew install --cask inkscape
sudo tlmgr install svg transparent trimspaces pagecolor
# or: brew install --cask mactex-no-gui   # bundles all LaTeX packages
```

If HTML renders cleanly but PDF fails, the break is usually environmental
(missing LaTeX package or Inkscape), not a code bug. Check
`/tmp/<name>.log` for the LaTeX error before touching the template.

### 7. Documentation audit

Per Workflow Rules 5, 6, and 7, every user-facing change since the previous
release must be reflected in:

- `docs/functions.md` — full function reference
- `docs/quickref.md` — must match registered builtins in
  `crates/rustlab-script/src/eval/builtins.rs`
- `AGENTS.md` — "All builtin functions" table, grammar, crate details
- `README.md` — if user-visible CLI flags, commands, or install steps changed
- REPL `help` entries — `HelpEntry` plus toolbox assignment in the
  `CATEGORIES` table of `crates/rustlab-cli/src/commands/repl.rs`

Find the previous release commit (`git log --oneline | grep -i 'bump to v' | head -1`)
and diff forward:

```sh
PREV=$(git log --oneline --grep='bump to v' | head -1 | awk '{print $1}')
git diff $PREV..HEAD -- crates/rustlab-script/src/eval/builtins.rs \
                        crates/rustlab-dsp/src \
                        crates/rustlab-script/src/eval/value.rs
```

Confirm every added builtin, DSP function, and `Value` variant has docs + REPL
help. Once `v0.1.7` is tagged, future releases can use `git diff v0.1.7..HEAD`
and tag ranges going forward.

### 8. Bump the version

The workspace uses a single version in the root `Cargo.toml`; all crates
inherit it via `version.workspace = true`, so only one file changes.

```sh
# Edit Cargo.toml → [workspace.package] → version = "0.1.X"
cargo build --workspace          # refreshes Cargo.lock with the new version
```

Stage `Cargo.toml` and `Cargo.lock` together — a lockfile mismatch will trip
CI on clones.

### 9. Commit, tag, and publish (only after user approval)

Match the existing commit-message convention in `git log`: a short imperative
summary of what is in the release, ending with `bump to v0.1.X`. Release-prep
changes (docs sync, version bump, small release-time fixes) may ride in the
same commit; unrelated feature work must not.

```sh
git add Cargo.toml Cargo.lock docs/ AGENTS.md README.md
git commit -m "<release summary>, bump to v0.1.X"
git tag -a v0.1.X -m "rustlab v0.1.X"
```

Ask the user before pushing. Then:

```sh
git push origin main
git push origin v0.1.X
```

After the tag is pushed, create a GitHub release with notes generated from
the commit range:

```sh
gh release create v0.1.X \
    --title "rustlab v0.1.X" \
    --generate-notes
```

Edit the notes if the auto-generated summary misses anything meaningful.
Attaching prebuilt binaries is optional; if attached, build them locally with
`make release` and upload with `gh release upload v0.1.X target/release/rustlab ...`.

### 10. Post-release smoke test

After the tag and GitHub release are live, install from the tag and confirm
the release binary works on a clean environment:

```sh
make install
rustlab --version                # must print the new version
rustlab run examples/dsp/lowpass.rlab   # must exit 0 with a plot
```

### Release rules (do not violate)

- **Never force push.** Not to `main`, not to a tag, not under any argument.
  If a tag was pushed incorrectly, delete the remote tag with user approval
  (`git push --delete origin v0.1.X`) and push a corrected tag — do not
  rewrite history on a shared ref.
- **Never skip hooks** (`--no-verify`) or bypass signing on a release commit.
- **Never tag a dirty tree** — every file in `git status` must be either
  committed or explicitly intended to be excluded.
- **Release-prep only in the bump commit.** Docs, version bump, and
  release-time fixes may share the commit; unrelated in-flight feature work
  must not.

---

## Crate Details

### `rustlab-core`

**Purpose:** Shared numeric types and traits. Zero internal dependencies.

**Key files:**
- `src/types.rs` — type aliases: `C64 = Complex<f64>`, `CVector = Array1<C64>`, `CMatrix = Array2<C64>`, `RVector = Array1<f64>`, `RMatrix = Array2<f64>`
- `src/traits/filter.rs` — `Filter` trait: `apply(&CVector)`, `frequency_response(n_points)`
- `src/traits/transform.rs` — `Transform` trait: `forward`, `inverse`
- `src/traits/decompose.rs` — `Decomposable` trait + marker traits `LuDecomposable`, `SvdDecomposable`, `CholeskyDecomposable`, `EigenDecomposable` (stubs — no implementors yet)
- `src/error.rs` — `CoreError` enum
- `src/sparse_solve/` — hand-rolled sparse direct solvers (per `dev/plans/closed/sparse_solve_handroll.md`, Davis-2006). `csc.rs` defines `SparseCsc<T>` and the `SparseScalar` trait (`f64` + `Complex<f64>` impls); `ordering.rs` has `OrderingMethod` trait + `ColCountOrdering`, `IdentityOrdering`, `AmdOrdering` (basic minimum-degree; full external-degree variant is deferred); `elimination_tree.rs` builds the column elimination tree; `cholesky.rs` is the up-looking sparse Cholesky for SPD matrices; `lu.rs` is the Gilbert-Peierls sparse LU with partial pivoting. `SparseMat::is_hermitian` and `SparseMat::is_spd_estimate` are pre-filter helpers used by the dispatch in `builtin_spsolve`. **End-to-end algorithm walkthrough:** `docs/sparse_solve.md` covers the dispatch chain (mode/ordering/realness resolution), the `OrderingHint` mechanism, the `SparseFactor` reuse path, and the underlying Davis algorithms with complexity and tolerance tables.
- `src/sparse_eig/` — hand-rolled sparse eigensolvers (Saad, *Numerical Methods for Large Eigenvalue Problems*, 2011). `lanczos.rs` builds a symmetric tridiagonal `T_m` via Lanczos with full reorthogonalization for Hermitian inputs; `arnoldi.rs` builds an upper-Hessenberg `H_m` via Arnoldi for general inputs; `sym_eig.rs` extracts eigenpairs from the small dense symmetric subproblem via cyclic Jacobi rotations; `hessenberg_eig.rs` does the same for non-symmetric subproblems via shifted QR + inverse iteration. Public `eigs(A, n, which, ...)` and `eigs_gen(A, B, n, which, ...)` entry points dispatch to the appropriate path.

**Feature flags:**
- `linalg` — enables optional `ndarray-linalg` dependency for future matrix decompositions

---

### `rustlab-dsp`

**Purpose:** DSP algorithms. Depends on `rustlab-core` only.

**Key files:**
- `src/window/mod.rs` — `WindowFunction` enum: `Rectangular`, `Hann`, `Hamming`, `Blackman`, `Kaiser { beta }`. Methods: `generate(length) -> RVector`, `from_str(s, beta)`
- `src/fir/design.rs` — `FirFilter` struct + `fir_lowpass`, `fir_highpass`, `fir_bandpass` (windowed-sinc method). `FirFilter` implements `Filter`.
- `src/iir/butterworth.rs` — `IirFilter { b: Vec<f64>, a: Vec<f64> }` + `butterworth_lowpass`, `butterworth_highpass` (bilinear transform, cascade of biquad sections). `IirFilter` implements `Filter`.
- `src/convolution.rs` — `convolve(x, h)` (direct O(nm)), `overlap_add(x, h, block_size)` (FFT-based)
- `src/vector_calc.rs` — 2-D: `gradient_2d(F, dx, dy)`, `divergence_2d(Fx, Fy, dx, dy)`, `curl_2d(Fx, Fy, dx, dy)`. 3-D: `gradient_3d(F, dx, dy, dz)`, `divergence_3d(Fx, Fy, Fz, dx, dy, dz)`, `curl_3d(Fx, Fy, Fz, dx, dy, dz)`. 2nd-order central interior + 2nd-order one-sided boundaries. 2-D operates on `CMatrix` (rows index y, cols index x); 3-D on `CTensor3` (axis 0 = y, axis 1 = x, axis 2 = z). Complex inputs throughout.
- `src/rasterize.rs` — Shape rasterization masks: `rect_mask(X, Y, x0, y0, w, h)`, `disk_mask(X, Y, xc, yc, r)`, `polygon_mask(X, Y, verts)` (even-odd ray casting / PNPOLY). All take meshgrid `X` / `Y` matrices and return a `CMatrix` of `0.0` / `1.0` the same shape. Compose with element-wise math.
- `src/laplacian.rs` — Sparse Laplacian builders: `laplacian_1d(n, dx, bc)`, `laplacian_2d_bc(nx, ny, dx, dy, bc)`, `laplacian_3d(nx, ny, nz, dx, dy, dz, bc)`, `laplacian_eps_2d(eps_map, dx, dy, bc)`. The `BoundaryCondition` enum (`Dirichlet | Neumann | Periodic`) is shared across all four; the parser at the builtin layer accepts the `"dirichlet"|"neumann"|"periodic"` string form. The `eps` variant uses harmonic-mean half-cell coefficients for flux conservation across material interfaces.
- `src/error.rs` — `DspError` (wraps `CoreError`)

---

### `rustlab-plot`

**Purpose:** Terminal charts, HTML export, and optional viewer client. Depends on `rustlab-core` only (viewer feature adds `rustlab-proto`).

**Contour subsystem (`src/contour.rs`):** Pure-functional helpers — `marching_squares(z, x, y, level)` returns line segments per level; `auto_levels(z, n)` picks Wilkinson-style round-number values from `{1, 2, 2.5, 5} × 10^k`; `band_index(value, levels)` classifies a value into a band for `contourf`'s per-cell SVG fill. Used by `builtin_contour` / `builtin_contourf`. Storage lives in `SubplotState.contours: Vec<ContourData>` so multiple contour layers can stack on a heatmap under `hold on`.

**Vector-field overlay subsystem (`src/quiver.rs`, `src/streamline.rs`):** Pure-functional helpers for 2-D vector-field plots. `quiver.rs` provides arrow geometry (`build_arrows`, `arrow_at`, `midpoint_arrow`) with `auto_scale` set so the longest arrow equals the nearest-neighbour cell distance; `streamline.rs` provides bilinear sampling and an RK4 forward+backward integrator with boundary clip and NaN termination. Used by `builtin_quiver` / `builtin_streamplot`. Storage lives in `SubplotState.quivers: Vec<QuiverData>` and `SubplotState.streamlines: Vec<StreamlineData>`, cleared on `hold off` but not `imagesc` / `contour`, so vector overlays stack naturally on heatmaps and contours.

**Key files:**
- `src/ascii.rs` — `plot_real`, `plot_complex`, `stem_real`, and the shared `draw_subplots(f, subplots, rows, cols)` helper used by both `render_figure_terminal` and `LiveFigure::redraw`.
- `src/live.rs` — `LiveFigure` struct implementing the `LivePlot` trait: `new(rows, cols)`, `update_panel(idx, x, y)`, `set_panel_labels(idx, title, xlabel, ylabel)`, `redraw()`. `Drop` impl restores the terminal.
- `src/figure.rs` — `FigureState`, `FIGURE` thread-local, and the multi-figure store (`FigureStore`). `figure_new()`, `figure_new_html(path)`, `figure_switch(id)` manage figure handles. Each figure tracks its own `FigureOutput` mode (Terminal, Html, or Viewer). The swap approach keeps a single active `FIGURE` workspace with inactive figures stored in a HashMap.
- `src/html.rs` — `render_figure_html(path)`: exports current FIGURE state to a self-contained HTML file with Plotly.js (CDN). Also provides HTML figure mode (`set_html_figure_path`, `sync_html_file`, `html_figure_active`) where `figure("file.html")` causes all subsequent plot commands to auto-update the HTML file instead of rendering to the terminal.
- `src/viewer_client.rs` — (feature `viewer`) thin Unix socket client for communicating with `rustlab-viewer`. Supports `connect()` (default socket) and `connect_named(name)` (named session socket).
- `src/viewer_live.rs` — (feature `viewer`) `ViewerFigure` implementing `LivePlot`, routes live plot data to the viewer over IPC. Also provides `connect_viewer()`, `connect_viewer_named(name)`, `disconnect_viewer()`, `viewer_active()`, `sync_viewer()` for routing regular (non-live) plot commands to the viewer when `viewer on` is active. Figure IDs use PID-based prefixes (`(pid << 16) | counter`) to avoid collisions when multiple rustlab processes connect to the same viewer. **Dead-connection recovery:** `sync_viewer()` detects write failures (viewer closed/crashed), clears the `VIEWER_CONN`/`VIEWER_SESSION` thread-locals, resets `FigureOutput` to `Terminal`, prints a warning to stderr, and re-renders the current figure in the TUI — subsequent plots keep using the terminal until another `viewer on`.

**Trait:** `LivePlot` (in `lib.rs`) — backend-agnostic interface for live plots. Implemented by `LiveFigure` (ratatui) and `ViewerFigure` (egui viewer). The script engine stores `Box<dyn LivePlot>` in `Value::LiveFigure`.

**Behavior:** Static plot functions enter the ratatui alternate screen, draw a braille-pixel chart, wait for a keypress, then restore the terminal. `LiveFigure` keeps the alternate screen open across multiple `redraw()` calls and only restores on `Drop`. When the `viewer` feature is enabled and `rustlab-viewer` is running, `figure_live()` automatically connects to the viewer instead of using ratatui. Neither should be called in non-TTY contexts (`render_figure_terminal` silently skips; `LiveFigure::new` returns `Err(PlotError::NotATty)`).

**Plot-output context (`PlotContext`):** Three variants — `Terminal` (default, TUI rendering), `Notebook` (silent assignments, figure snapshots for the notebook executor), and `Headless` (no TUI). Under `Headless`, `render_figure_terminal()` and `imagesc_terminal()` short-circuit, and `LiveFigure::new()` returns `PlotError::HeadlessDisabled`. The CLI `rustlab run --plot none` sets `Headless`; `rustlab run --plot viewer [--viewer-name NAME]` calls `connect_viewer()` at startup and leaves the context at `Terminal` so per-figure viewer routing takes over. The notebook/figure-snapshot and silent-assignment behaviors remain keyed on `Notebook` only — `Headless` does not inherit them.

---

### `rustlab-proto`

**Purpose:** Wire protocol for rustlab ↔ rustlab-viewer IPC. Messages are length-prefixed msgpack.

**Key types:**
- `ViewerMsg` — client→viewer messages: `FigureOpen`, `PanelUpdate`, `PanelLabels`, `PanelLimits`, `Redraw`, `Close`, `Ping`
- `ViewerReply` — viewer→client replies: `Ok`, `Error`, `Pong`
- `WireSeries` — data series with `x`, `y`, `color`, `style`, `kind`, and optional `x_labels` for categorical bar charts
- `default_socket_path()` — `/tmp/rustlab-viewer-{uid}.sock` (overridden by `$RUSTLAB_VIEWER_SOCK`)
- `socket_path_for_name(name)` — `/tmp/rustlab-viewer-{uid}-{name}.sock` for named sessions

---

### `rustlab-viewer`

**Purpose:** Standalone egui plot viewer. Receives plot data from rustlab over a Unix socket and renders interactive charts with zoom, pan, crosshairs, and point readout.

**Usage:**
```
rustlab-viewer                  # default session
rustlab-viewer --name work      # named session (separate socket)
rustlab-viewer --socket PATH    # custom socket path
```

**Key files:**
- `src/main.rs` — CLI arg parsing, eframe GUI launch, `--name`/`--socket` support
- `src/app.rs` — `ViewerApp` eframe application, drains messages from socket, renders figures in egui windows
- `src/figure.rs` — `FigureWindow` and `PanelState`, subplot grid rendering with `egui_plot`, categorical x-axis label support
- `src/net.rs` — Unix socket listener, spawns per-connection threads, liveness check prevents clobbering an active viewer's socket
- `src/render.rs` — converts `WireSeries` to egui_plot items (Line, Points, BarChart, Stem)

**Multi-instance design:**
- Multiple viewers can run simultaneously using `--name` (each gets its own socket)
- Multiple rustlab processes can connect to the same viewer — PID-based figure IDs prevent collisions
- Starting a second viewer on the same socket is blocked with a liveness ping check

---

### `rustlab-script`

**Purpose:** Interpreter for `.rlab` script files and the REPL. Depends on core, dsp, and plot.

**Key files:**
- `src/lexer.rs` — hand-written lexer → `Vec<Spanned<Token>>`
- `src/parser.rs` — recursive-descent parser → `Vec<Stmt>`
- `src/ast.rs` — `Stmt` (Assign, Expr, FunctionDef, FieldAssign, Return, Hold, Grid, Viewer, For, While, IndexAssign, ...), `Expr` (Number, Str, Var, BinOp, UnaryMinus, Call, Matrix, Range, Transpose, Field, Lambda, FuncHandle, CellArray), `BinOp`
- `src/eval/mod.rs` — `Evaluator` struct: holds `env`, `builtins`, `user_fns`, `in_function`, `profiler: profile::Profiler`; public API: `run()`, `run_script()`, `enable_profiling()`, `has_profile_data()`, `take_profile()`
- `src/eval/value.rs` — `Value` enum: `Scalar(f64)`, `Complex(C64)`, `Vector(CVector)`, `Matrix(CMatrix)`, `Str(String)`, `StringArray(Vec<String>)`, `Struct(HashMap<String,Value>)`, `Bool(bool)`, `Lambda { params, body, captured_env }`, `FuncHandle(String)`, `QFmt`, `FirState(Arc<Mutex<Vec<C64>>>)`, `AudioIn { sample_rate, frame_size }`, `AudioOut { sample_rate, frame_size }`, `LiveFigure(Arc<Mutex<Option<Box<dyn rustlab_plot::LivePlot>>>>)`, `All`, `None`
- `src/eval/builtins.rs` — `BuiltinRegistry`: `HashMap<String, BuiltinFn>` where `BuiltinFn = fn(Vec<Value>) -> Result<Value, ScriptError>`
- `src/eval/toml_io.rs` — TOML import/export: `save_toml()`, `load_toml()`, and `Value ↔ toml::Value` converters
- `src/eval/profile.rs` — `Profiler` struct (opt-in, zero overhead when disabled); `print_report()` prints table to stderr
- `src/lib.rs` — public entry points: `run(source)`, `run_profiled(source)`

**Pre-populated environment constants:** `j = Complex(0,1)`, `i = Complex(0,1)`, `pi = 3.14159…`, `e = 2.71828…`, `Inf = f64::INFINITY`, `NaN = f64::NAN`, `true = Bool(true)`, `false = Bool(false)`

**`BUILTIN_CONSTS`:** These constant names (`i`, `j`, `pi`, `e`, `Inf`, `NaN`, `true`, `false`) survive `clear_vars()` — they are re-inserted automatically so the REPL never loses them.

**How `Call` nodes are evaluated:** At eval time, if the name exists in `env` as a `Vector`, `Matrix`, `Tuple`, `Str`, or sparse variant, it is treated as 1-based indexing — `end` is temporarily bound to the container length. String indexing returns a string: `s(3)` → single char, `s(1:5)` → substring, `s(:)` → full copy. If the name holds a `Lambda`, it is called with its captured environment. Otherwise it is a `BuiltinRegistry` call.

**Lambda / anonymous functions:** `@(x, y) expr` creates a `Value::Lambda` that captures the current env by snapshot. `@name` creates a `Value::FuncHandle` that lazily resolves to a lambda clone (if `name` holds a lambda) or dispatches to a builtin/user function. `arrayfun(f, v)` maps any callable over a vector, returning a `Vector` (all scalar outputs) or a `Matrix` (all vector outputs of equal length). `feval("name", args...)` calls a function by string name.

**Profiling:** `profile(fn1, fn2)` inside a script enables selective tracking of named functions. `profile()` with no args tracks all calls. `profile_report()` prints a mid-script report to stderr. `--profile` CLI flag (on `rustlab run`) tracks all calls without modifying the script. `Profiler` uses a `higher_order_depth` counter so inner callbacks inside `arrayfun` or user functions are not recorded individually — only the outer call's total time is captured. Zero overhead when disabled.

**Adding a new builtin function:**
1. Write `fn builtin_foo(args: Vec<Value>) -> Result<Value, ScriptError>` in `src/eval/builtins.rs`
2. Add `r.register("foo", builtin_foo);` in `BuiltinRegistry::with_defaults()`
3. No parser or grammar changes required

---

### `rustlab-cli`

**Purpose:** Binary crate. Wires clap subcommands to the other crates.

**Key files:**
- `src/main.rs` — calls `Cli::parse().execute()`
- `src/cli.rs` — `Cli` struct with `Option<Commands>` (None → REPL)
- `src/commands/repl.rs` — interactive REPL using `rustyline`; persistent `Evaluator` across inputs
- `src/commands/run.rs` — reads a file, calls `rustlab_script::run`. Supports `--profile` and `--plot {tui|none|viewer} [--viewer-name NAME]` (see `PlotContext` note under `rustlab-plot`).
- `src/commands/filter.rs` — `fir` and `iir` subcommands
- `src/commands/convolve.rs` — reads CSV signals, calls `convolve` or `overlap_add`
- `src/commands/window.rs` — generates window, prints values, optional `--plot`
- `src/commands/plot.rs` — reads CSV, dispatches to plot functions
- `src/commands/docs.rs` — `rustlab docs` subcommand. Surfaces the REPL's `HELP` array and `CATEGORIES` toolbox table (`pub` items in `commands/repl.rs`) from the shell. Forms: `docs` (list-all grouped by toolbox), `docs <name>` (detail), `docs <toolbox>` (single-toolbox list — e.g. `docs dsp`, `docs rf`), `docs <subcategory>` (cross-toolbox subcategory match), `docs --search <q>` (substring match over names+briefs), `docs --json` (machine-readable dump with `toolbox` + `subcategory` fields). Unknown topic exits non-zero with a "No help found" message. Tests in `tests/docs.rs`.

**Default behaviour:** `rustlab` with no arguments starts the REPL.

### `rustlab-notebook`

**Purpose:** Library + binary crate. Renders Markdown notebooks with \`\`\`rustlab code blocks into self-contained HTML, LaTeX, or PDF.

**Key files:**
- `src/lib.rs` — public API: `cmd_render`, `cmd_render_dir` (accepts optional `index_title`), `Format`, `NotebookNav`, `generate_index_html` (accepts an `index_body_html: &str`); contains `render_output` (per-format dispatch) and `plot_layout_for` (canonical `plots/<stem>/` rule shared by markdown + LaTeX)
- `src/main.rs` — thin CLI wrapper (`rustlab-notebook render`)
- `src/parse.rs` — parse notebook markdown into `Block` enum (Markdown / Code / Mermaid / Callout / Exercise / Solution)
- `src/execute.rs` — execute code blocks through `Evaluator`, produce `Rendered` blocks
- `src/render.rs` — HTML rendering with themed CSS (Catppuccin Mocha/Latte)
- `src/render_latex.rs` — LaTeX rendering (also used to drive PDF compilation in a tempdir)
- `src/render_markdown.rs` — GitHub-flavored Markdown rendering with inline SVG plots
- `src/mermaid.rs` — pure-Rust SVG rendering of ` ```mermaid ` blocks via `mermaid-rs-renderer` (gated behind the default-on `mermaid` Cargo feature). BLAKE3-hashed output cache lives under `plots/<notebook>/.cache/`. Wraps the upstream call in `catch_unwind` so a 0.2.x crate panic falls back to verbatim source instead of tearing down the render.

**Theme support:** `--theme dark|light` flag (default: dark). Dark = Catppuccin Mocha, Light = Catppuccin Latte. Theme colors are defined in `rustlab-plot/src/theme.rs` (`Theme` enum + `ThemeColors` struct) and shared with the plot crate for consistent Plotly chart styling.

**Multi-plot blocks:** Each `savefig()` call inside a code block captures a separate `FigureState` snapshot (via `rustlab_plot::push_notebook_figure_snapshot`, hooked into `render_figure_file` when `PlotContext::Notebook` is active). If a block plots but never calls `savefig()`, a single final snapshot is taken. `Rendered::Code.figures` is a `Vec<FigureState>`.

**Math protection (`render::protect_math` / `restore_math`):** CommonMark consumes `\\` → `\`, which would destroy LaTeX row separators inside `$$...$$` (e.g. matrix `\\` row breaks would collapse). Before calling `pulldown-cmark`, both the `Markdown` and `Callout` branches stash math spans (`$$...$$` and KaTeX-strict `$...$`) under Unicode private-use placeholders, then restore the originals after `push_html`. Authors write standard LaTeX (`\\`, `\$`, `\begin{pmatrix}…\end{pmatrix}`) — no double-escaping. Code fences and inline code spans are skipped, and `\$` escapes are honored.

**Silent assignments:** Under `PlotContext::Notebook`, the `Evaluator` suppresses assignment echo (via `echo_enabled()`). Only bare expressions, `print()`, and `disp()` produce visible text output — matching Jupyter notebook conventions. REPL and `rustlab run` behaviour is unchanged.

**YAML frontmatter (`--- title: ... ---`):** Parsed by `parse::extract_frontmatter` → `Frontmatter { title, order }`. Known keys are `title` (overrides the `# H1` fallback in `extract_title`) and `order` / `weight` (signed integer; sorts entries on the directory index page, ascending, ties broken by filename). Unknown keys are ignored silently so future additions don't break existing files. Quoted values (single or double) are unwrapped.

**Directory index page (`cmd_render_dir`):** Generates `index.html` listing every rendered notebook. Title precedence: `--title <STRING>` CLI flag > `index.md`'s H1/frontmatter title > parent directory name. When `index.md` is present, it is excluded from the notebook list (it IS the index) and its markdown body is rendered as plain HTML above the list — *without* executing any code fences (the index page is kept dependency-free; put executable content in regular notebooks and link to them from `index.md`). Entries are sorted by frontmatter `order` ascending, ties by filename; entries without `order` sort after those that have one.

**Cross-notebook page navigation (`NotebookNav`):** In directory-mode HTML renders, each notebook receives a `NotebookNav { index_href, prev, next }` computed from the sorted entry list. `render::render_html` switches the page layout when `nav` is `Some`: the fixed sidebar (with per-page H1/H2/H3 TOC) is dropped, the `<body>` gets a `topbar-layout` class, and a sticky `<header class="topbar">` breadcrumb (`← Index / <title>`) is inserted at the top. A `Previous · Index · Next` footer bar is appended above the page footer. Single-file `cmd_render` passes `None` → standalone pages keep the sidebar + TOC layout unchanged. LaTeX/PDF renders ignore `NotebookNav`.

**Accessible via:** the standalone `rustlab-notebook` binary (`rustlab-notebook render ...`, `rustlab-notebook watch ...`, etc.). The main `rustlab` CLI deliberately does **not** ship a `notebook` subcommand — keeps the primary binary small (no clap notebook surface, no `rustlab-notebook` link dependency). `--title` is directory-mode only; passing it with a single-file input prints a warning and is ignored.

**Renderer design pattern — split write location from reference path:**

Every emitter that writes plot files to disk (`render_markdown`, `render_latex`, and any future format) takes **two** arguments for plots, not one:

- `plot_dir: &Path` — where the SVG bytes are written
- `plot_href_prefix: &str` — what relative path is embedded in the rendered document (markdown `![alt](…)`, LaTeX `\includesvg{…}`, etc.)

These are intentionally distinct. The on-disk dir is a host-filesystem path; the href is a string the rendered document carries to its eventual reader. Conflating them (e.g. deriving the href from `plot_dir.file_name()`) hardcodes the on-disk layout into the document and prevents the caller from picking a different one.

Both renderers are then wired through `lib.rs::plot_layout_for(out_path)`, which produces the canonical pair `(out_dir/plots/<stem>/, "plots/<stem>")`. That single rule gives every format the same on-disk shape: one document file plus one subdirectory under a shared `plots/` umbrella, scaling cleanly to directory-mode renders of many notebooks.

Self-contained formats (HTML embeds plots inline; PDF compiles in a tempdir and copies only the `.pdf` out) skip the layout helper entirely — the user asked for one file, so they get one file. The tempdir for PDF is the same idea applied to *all* intermediates, not just plots: the `.tex`, the SVGs, and pdflatex's `aux/log/out` sidecars all live in the temp dir and disappear on success. On compile failure, only the build log is copied to `<pdf_path>.log` so it survives the cleanup.

When adding a new output format, follow this pattern:

- If the format is self-contained (single-file deliverable), generate intermediates in a tempdir and copy only the final artifact out.
- If the format references external plot files, take both `plot_dir` and `plot_href_prefix`, and call `plot_layout_for(out_path)` from `render_output` to get the canonical pair.

Documented for users in `docs/notebooks.md` ("Plot output layout").

---

## Scripting Language Reference

Scripts use the `.rlab` extension. Run with `rustlab run script.rlab` or enter statements interactively in the REPL.

### Grammar (informal)

```
program     = stmt*
stmt        = IDENT ("=" | "+=" | "-=" | "*=" | "/=") range_expr [";"] "\n"  # assignment
            | IDENT "(" arglist ")" "=" range_expr [";"] "\n"  # indexed assignment
            | IDENT "." IDENT "=" range_expr [";"] "\n"    # struct field assignment
            | range_expr [";"] "\n"                         # expression
            | "function" [IDENT "="] IDENT "(" params ")"  # function definition
                stmt* "end"
            | "return" [";"] "\n"                          # early return (inside function)
            | "if" range_expr [","|"\n"]                    # conditional
                stmt* ["elseif" range_expr stmt*]*
                ["else" stmt*] "end"
            | "for" IDENT "=" range_expr "\n"              # for loop
                stmt* "end"
            | "while" range_expr "\n"                      # while loop
                stmt* "end"
            | "switch" range_expr                         # switch/case
                ("case" range_expr stmt*)*
                ["otherwise" stmt*] "end"
            | "run" FILEPATH [";"] "\n"                    # execute .rlab script
            | "format" IDENT [";"] "\n"                    # display mode (commas, default)
            | "#" ... "\n"                                  # comment
            | "..." ... "\n"                                # line continuation

range_expr  = expr (":" expr (":" expr)?)?     # a:b or a:step:b → Vector

expr        = term (("+"|"-") term)*
term        = factor (("*"|"/"|".*"|"./") factor)*
factor      = postfix (("^"|".^") factor)?     # right-associative
postfix     = primary ("'" | ".'" | "." IDENT ["(" arglist ")"] | "(" arglist ")")*
                # ' = conjugate transpose; .' = plain transpose
                # .field = struct access; .method(args) = method-call sugar
                # (args) after any non-Var expr = chained index: f(a)(i)

primary     = NUMBER | STRING | IDENT
            | IDENT "(" range_arglist ")"       # call or 1-based index
            | "[" range_row (";" range_row)* "]"
            | "{" expr ("," expr)* "}"          # string array literal
            | "(" range_expr ")"
            | "-" primary
            | "@" "(" params ")" expr           # anonymous function (lambda)
            | "@" IDENT                         # function handle
```

### Key language behaviours

| Feature | Syntax | Notes |
|---|---|---|
| Imaginary unit | `j` | Predefined constant `Complex(0,1)` |
| Complex number | `1.5 + j*2.0` | Standard arithmetic |
| Compound assign | `x += 1`, `-=`, `*=`, `/=` | Desugared to `x = x op expr` in parser |
| Suppress output | `x = expr;` | Trailing `;` on any statement |
| Range | `1:10`, `0:0.5:2`, `10:-1:1` | Creates a real `Vector` |
| 1-based index | `v(3)`, `v(2:5)`, `v(end)` | `end` = `len(v)`; slice returns Vector |
| Indexed assign (single element) | `v(i) = val`, `M(r,c) = val` | Vectors auto-created/grown; matrices must exist |
| Region assign — matrix | `M(i, :) = vec`, `M(:, j) = vec`, `M(rows, cols) = mat`, `M(:, :) = scalar` | Row / column / submatrix / broadcast writes. Indices may be Scalar, `:`, or Vector. RHS must match the target region shape; scalar/complex RHS broadcasts. Sparse matrices: only single-element form supported. Added in 0.3.4. |
| Region assign — vector | `v(:) = vec`, `v([1,3,5]) = vec`, `v(1:2:end) = vec`, `v(idx) = scalar` | Strided / explicit-list LHS into a Vector. RHS length must match the index count; scalar/complex RHS broadcasts. Added in 0.3.4. |
| Chained index | `f(a,b)(i)` | Index return value of any call without a temp variable |
| If / elseif | `if cond ... elseif cond2 ... else ... end` | Chained conditionals; single-line: `if cond, body; end` |
| Switch / case | `switch expr case v1 ... otherwise ... end` | Match value against cases; first match wins |
| For loop | `for i = 1:n ... end` | Iterates over range or vector; loop var stays in scope |
| While loop | `while cond ... end` | Repeats body while cond is truthy; cond may be Bool, Scalar (nonzero), or Complex |
| ~~`break` / `continue`~~ | **Not supported — by design.** | Find-first patterns: lift the exit condition into the `while` header (`while i <= N && !found`). Skip patterns: invert the `if` predicate. See `dev/requests/break-continue.md`. Do not generate `break` or `continue`. |
| Run (include) | `run file.rlab` | Execute a .rlab script; merges variables and functions into current scope |
| Line continuation | `x = a + ...` (newline) `  b` | `...` skips rest of line; statement continues on next line |
| Single-quote strings | `'hello'` | Alternative string delimiters; context-dependent (transpose after `)`, `]`, ident, number) |
| String indexing | `s(3)`, `s(1:5)`, `s(:)` | 1-based; returns string; `end` supported |
| Clear workspace | `clear` | Bare command (no parens); removes all user vars/fns, keeps built-in constants |
| Clear figure | `clf` | Bare command (no parens); resets figure state (equivalent to `figure()`) |
| Hold/Grid/Viewer | `hold on`, `grid off`, `viewer on` | Bare keyword commands; also accept function-call form `hold("on")` |
| Viewer status | `viewer` | Bare `viewer` (no arg) reports connection state + current figure routing (rustlab-viewer / HTML file / TUI) |
| Lambda | `f = @(x) x^2` | Creates anonymous function; captures env by snapshot at creation |
| Function handle | `@sin`, `@myFn` | Reference to builtin or user-defined function |
| Higher-order | `arrayfun(@sin, v)` / `parmap(@sin, v)` | Maps callable over vector. `arrayfun` is sequential; `parmap` is parallel via rayon (1-D iterables, scalar outputs in v1) — see `dev/plans/parmap_parreduce.md`. |
| Dynamic call | `feval("name", args...)` | Call function by string name |
| Profile | `profile(fn1, fn2)` / `profile()` | Track named functions (or all); `profile_report()` prints mid-script |
| Concatenation | `[v1, v2]` | Vectors inside `[...]` are flattened |
| Transpose | `v'` | Conjugate transpose |
| Element-wise | `.*` `./` `.^` | Always element-wise on vectors/matrices |
| Matrix literal | `[1,2; 3,4]` | `;` separates rows |
| Sparse types | `SparseVector`, `SparseMatrix` | COO format; 0-based internal, 1-based in script; auto-promote to dense in binops |
| Rank-3 tensor | `Value::Tensor3` — shape `(m, n, p)` | Built via `zeros3`/`ones3`/`rand3`/`randn3`/`reshape(A, m, n, p)`/`cat(3, ...)`. 1-based indexing `A(i,j,k)`; `A(:,:,k)` returns a Matrix (trailing singleton dropped). No broadcasting between Matrix and Tensor3; no `*`/`/` between two Tensor3s (use `.*`/`./`). Column-major reshape walk. See `dev/plans/closed/tensor3.md` for the full design. |
| String array | `{"a", "b", "c"}` | `Value::StringArray`; all elements must be strings; 1-based indexing |
| Underscore literals | `1_000_000`, `3.141_592` | Digit separators stripped at lex time; like Rust/Python/C++ |
| Format mode | `format commas` / `format default` | Bare command; toggles thousands separators in auto-print output |

### All builtin functions

| Function | Signature | Returns |
|---|---|---|
| `abs` | `abs(x)` | Magnitude (element-wise) |
| `angle` | `angle(x)` | Phase in radians (element-wise) |
| `real` | `real(x)` | Real part |
| `imag` | `imag(x)` | Imaginary part |
| `cos` | `cos(x)` | Cosine (element-wise) |
| `sin` | `sin(x)` | Sine (element-wise) |
| `sqrt` | `sqrt(x)` | Square root (element-wise) |
| `exp` | `exp(x)` | e^x (element-wise) |
| `log` | `log(x)` | Natural log (element-wise) |
| `zeros` | `zeros(n)` / `zeros(n, m)` | Complex zero vector of length n, or n×m zero matrix |
| `ones` | `ones(n)` / `ones(n, m)` | Complex ones vector of length n, or n×m ones matrix |
| `linspace` | `linspace(start, stop, n)` | Real vector of n points |
| `rand` | `rand()` / `rand(n)` / `rand(m, n)` | Uniform U[0,1) scalar / vector / matrix. Zero-arg form added in 0.3.4. |
| `randn` | `randn()` / `randn(n)` / `randn(m, n)` | Standard-normal N(0,1) scalar / vector / matrix. Zero-arg form added in 0.3.4. |
| `randi` | `randi(imax)` / `randi(imax, n)` / `randi([lo,hi], n)` | Integer scalar or vector drawn uniformly |
| `seed` | `seed(N)` / `seed()` | Re-seed the shared RNG with a non-negative integer for reproducible `rand`/`randn`/`randi`/`rand3`/`randn3`/`sprand` sequences. `seed()` (no args) re-seeds from OS entropy. Notebook authors who commit rendered SVG/MD should call `seed(N)` near the top to keep re-renders bit-stable. |
| `len` | `len(v)` | Number of elements |
| `length` | `length(v)` | Alias for `len`. Scalar / complex / bool return `1`; matrix returns `max(size(M))`; Tensor3 returns the longest axis. The `length(scalar)` form was added in 0.3.4 (previously errored) so generic helpers can call `length(x)` without `length([x])` boxing. |
| `numel` | `numel(x)` | Total elements (rows×cols for matrices, m·n·p for tensor3) |
| `size` | `size(x)` / `size(x, dim)` | `[rows, cols]` or `[m, n, p]` (tensor3) as a Vector; `size(x, 3)` valid only for tensor3 |
| `ndims` | `ndims(x)` | 3 for tensor3, 2 otherwise (Octave convention) |
| `zeros3` | `zeros3(m, n, p)` / `zeros3([m, n, p])` | Rank-3 complex zero tensor |
| `ones3` | `ones3(m, n, p)` | Rank-3 complex ones tensor |
| `rand3` | `rand3(m, n, p)` | Rank-3 tensor, U[0, 1) samples |
| `randn3` | `randn3(m, n, p)` | Rank-3 tensor, N(0, 1) samples |
| `cat` | `cat(dim, A, B, ...)` | Concatenate along dim 1 (rows) / 2 (cols) / 3 (pages → tensor3) |
| `permute` | `permute(A, [d1, d2, d3])` | Reorder tensor3 axes; `order` is a permutation of `[1, 2, 3]` |
| `squeeze` | `squeeze(A)` | Drop singleton dimensions from a tensor3 (→ Matrix / Vector / Scalar) |
| `print` | `print(x)` | Print to stdout |
| `plot` | `plot(x)` | Terminal line chart (blocks until keypress) |
| `stem` | `stem(x)` | Terminal stem chart (blocks until keypress) |
| `window` | `window(name, n)` | Real window vector |
| `pwelch` | `pwelch(x, fs)` / `pwelch(x, fs, win)` / `pwelch(x, fs, win, noverlap)` / `pwelch(x, fs, win, noverlap, nfft)` / `pwelch(x, fs, win, noverlap, nfft, sided)` / `[Pxx, f] = pwelch(...)` | Welch's PSD estimator. MATLAB-compat defaults: Hamming window of length `floor(2·length(x)/9)` (8 segments at 50% overlap), `nfft = win_len` (rounded up to next power of two), no detrending, `sided = "auto"` (one-sided for real input, two-sided for complex). `win` accepts a string name, an integer length (Hamming of that length), or a real coefficient vector. Returns `[Pxx, f]` tuple; bare call auto-plots `10·log10(Pxx)` vs Hz on the current subplot. Phase 1 of `dev/plans/time_frequency.md`. |
| `stft` | `stft(x, fs)` / `stft(x, fs, win, noverlap, nfft, sided)` / `[S, f, t] = stft(...)` | Short-Time Fourier Transform. MATLAB-compat defaults: Hann window of length **128**, 50% overlap, `nfft = win_len`, `sided = "auto"`. `S` is a complex matrix with rows indexed by frequency (low at row 1) and columns indexed by time frame (early at col 1). `f` Hz, `t` seconds (segment centres). Reuses `segment_iter` from Phase 1. Bare call auto-renders the magnitude spectrogram. Phase 2 of `dev/plans/time_frequency.md`. |
| `spectrogram` | `spectrogram(x, fs)` / `spectrogram(x, fs, win, noverlap, nfft, sided)` | Plot-only wrapper around `stft`. Renders `20·log10(|S|)` via the shared `rustlab_plot::db_clip` helper (80 dB dynamic range) + `imagesc` with `viridis` colormap, `axis("xy")` (frequency increases upward), and "Time (s)" / "Frequency (Hz)" labels. No data return — for the underlying matrix use `[S, f, t] = stft(x, fs, ...)`. Phase 2 of `dev/plans/time_frequency.md`. |
| `waterfall` | `waterfall(x, fs)` / `waterfall(x, fs, win, noverlap, nfft, sided)` / `[W, f, t] = waterfall(...)` | Frequency waterfall: STFT magnitude in dB transposed and time-reversed so that **row 1 = newest segment** and column 1 = DC. `W` is a real `[n_time × n_freqs]` matrix; `t` is aligned with rows (monotonically decreasing). Argument forms mirror `stft`. Phase 1 of `dev/plans/waterfall.md` — data only; the combined two-panel live view (spectrum line on top, downward-scrolling heatmap below) lands in Phase 3 via `waterfall_stream`. |
| `cwt` | `cwt(x, fs)` / `cwt(x, fs, "morlet")` / `cwt(x, fs, "morlet", n_scales)` / `cwt(x, fs, "morlet", scales_vector)` / `[W, freqs, t] = cwt(...)` | Continuous Wavelet Transform with the analytic Morlet wavelet (ω₀ = 6). Frequency-domain implementation: zero-pad signal by `4·max_scale` on each side, FFT once, multiply by analytic Morlet's Gaussian transfer function per scale, IFFT, trim. Returns `W` (n_scales × len(x) complex), `freqs = ω₀·fs/(2π·scales)` per row, `t = (0..len)/fs`. Default scale grid: 64 log-spaced from 2 to `len(x)/4` samples. Phase 3 of `dev/plans/time_frequency.md`. |
| `scalogram` | `scalogram(x, fs)` / `scalogram(x, fs, "morlet", n_or_scales)` | Plot-only wrapper around `cwt`. Same `rustlab_plot::db_clip` + `imagesc` + `axis("xy")` pipeline as `spectrogram`. With default log-spaced scales the row-index y-axis is effectively logarithmic-frequency. Phase 3 of `dev/plans/time_frequency.md`. |
| `pwelch_stream_init` / `pwelch_stream` | `state = pwelch_stream_init(fs, win, noverlap, nfft [, ema_alpha])` / `[Pxx, state] = pwelch_stream(frame, state)` | Frame-by-frame Welch PSD. Cumulative running average by default (converges in distribution to batch `pwelch_psd`); pass `ema_alpha ∈ (0, 1]` to switch to an exponential moving average that tracks non-stationary signals. Returns empty during warmup. Phase 4 of `dev/plans/time_frequency.md`. |
| `stft_stream_init` / `stft_stream` | `state = stft_stream_init(fs, win, noverlap, nfft [, sided])` / `[S_cols, state] = stft_stream(frame, state)` | Frame-by-frame STFT. Returns whichever new spectrogram columns the new samples produced (`n_freqs × 0` when none). `sided` defaults to one-sided since streaming needs a fixed row count at init time. Phase 4. |
| `waterfall_stream_init` / `waterfall_stream` | `state = waterfall_stream_init(fs, win, noverlap, nfft, time_history [, vmin_db, vmax_db, colormap, smooth_frames, update_every])` / `[fig, state] = waterfall_stream(samples, fig, state)` | Combined-call streaming waterfall: pushes samples into a rolling `[n_time × n_freqs]` dB-magnitude buffer (`n_time = ceil(time_history*fs/hop)`, newest row at front), then on every `update_every`-th tick refreshes panel 1 (line spectrum, smoothed over `smooth_frames`) and panel 2 (heatmap with `HeatmapOrigin::Upper`, downward-scrolling). One-time panel labelling on first redraw; `fig` must be `figure_live(2, 1)`. Phase 3 of `dev/plans/waterfall.md`. |
| `cwt_stream_init` / `cwt_stream` | `state = cwt_stream_init(fs, n_samples [, n_scales \| scales])` / `[W, state] = cwt_stream(frame, state)` | Sliding-window streaming CWT. Each call recomputes `cwt_morlet` over the latest `n_samples` samples (older drop off the front). Edge effects on the rightmost columns are not trimmed (decision 13 in the plan). Phase 4. |
| `plot_update_heatmap` | `plot_update_heatmap(fig, panel, matrix [, colormap [, vmin, vmax]])` | Live heatmap counterpart of `plot_update`. Routes through the `LivePlot::update_panel_heatmap` trait method — ratatui paints colored cells; the viewer feature rasterizes through `rustlab_plot::render_panel_to_rgba` and sends a `PanelHeatmap` wire message (existing protocol — no proto bump). Always uses `HeatmapOrigin::Lower` (row 0 at the bottom — physics/`imagesc` convention); downward-scrolling waterfalls call into `update_panel_heatmap` directly with `HeatmapOrigin::Upper`. Pair with `plot_limits` for axis labelling. |
| `HeatmapOrigin` | `HeatmapOrigin::{Lower, Upper}` (Rust enum on `rustlab_plot::HeatmapData::origin`) | Where row 0 of a heatmap matrix lands in the rendered image. `Lower` (default — `imagesc`, `spectrogram`, `scalogram`) puts row 0 at the bottom; `Upper` (waterfall) puts row 0 at the top. Independent of `AxisYDirection`, which controls axis label orientation. Applied centrally in `render_heatmap_cells_to_rgba`. Phase 2 of `dev/plans/waterfall.md`. |
| `Value::DspStreamState` | `dsp_stream_state` (script type name) | Opaque handle wrapping a streaming-DSP state (pwelch / stft / cwt). One `Value` variant with an internal `DspStreamKind` enum dispatching per call (decision 14 in the plan). `Arc<Mutex<...>>` semantics match `FirState`: shared across destructuring re-binds. Phase 4. |
| `fir_lowpass` | `fir_lowpass(taps, cutoff_hz, sr, window)` | FIR coefficient Vector |
| `fir_highpass` | `fir_highpass(taps, cutoff_hz, sr, window)` | FIR coefficient Vector |
| `fir_bandpass` | `fir_bandpass(taps, low_hz, high_hz, sr, window)` | FIR coefficient Vector |
| `butterworth_lowpass` | `butterworth_lowpass(order, cutoff_hz, sr)` | IIR b-coefficient Vector |
| `butterworth_highpass` | `butterworth_highpass(order, cutoff_hz, sr)` | IIR b-coefficient Vector |
| `median` | `median(v)` | Median of real parts; scalar for odd length, average of two middles for even |
| `convolve` | `convolve(x, h)` | Convolved Vector (length = len(x)+len(h)-1) |
| `filtfilt` | `filtfilt(b, a, x)` | Zero-phase forward-backward filter; uses odd-reflection signal extension + steady-state IC (matches Octave); use `a=[1]` for FIR |
| `prod` | `prod(v)` | Product of all elements (Vector or Matrix); returns Scalar |
| `firpmq` | `firpmq(n_taps, bands, desired [, weights [, bits [, n_iter]]])` | Integer-coefficient Parks-McClellan; defaults bits=16, n_iter=8. Returns integer-valued taps. For unit-gain passband, `sum(h_int)` equals the scale factor — use `freqz(h_int / sum(h_int), ...)` to verify. |
| `arrayfun` | `arrayfun(f, v)` | Apply callable `f` to each element of `v`; scalar outputs → Vector, vector outputs → Matrix |
| `parmap` | `parmap(f, xs)` | Parallel map. Apply `f` (lambda or function handle) to each element of `xs` across the rayon thread pool. Output shape follows the lambda's return: scalar/complex/bool → Vector; length-`d` Vector → `(N, d)` Matrix (per-call index = row, matching `arrayfun`); `m×n` Matrix → `(m, n, N)` Tensor3 (per-call index = trailing pages axis, so `result(:, :, k)` extracts the k-th per-call matrix). All trials must return the same shape; mismatch hard-errors naming the divergent trial. Per-task RNG seeded deterministically from the calling thread's master seed — `seed(N); parmap(...)` is bit-reproducible. Pure-lambda contract: the body may NOT call plotting/file-I/O/audio/FirState/seed builtins. Errors cancel-and-propagate. See `dev/plans/parmap_parreduce.md` (v1) and `dev/plans/parmap_nonscalar_outputs.md` (vector/matrix outputs). |
| `nproc` | `nproc()` | Number of logical CPUs (via `std::thread::available_parallelism()`). Same number rayon's pool and `parmap` use. Respects cgroup limits on Linux; returns total cores on macOS/Windows. |
| `feval` | `feval("name", args...)` | Call function by string name |
| `profile` | `profile(fn1, ...)` / `profile()` | Enable selective (or all-function) call profiling in-script |
| `profile_report` | `profile_report()` | Print profiling table to stderr immediately |
| `logspace` | `logspace(a, b, n)` | n log-spaced points from 10^a to 10^b |
| `rk4` | `rk4(f, x0, t)` | Fixed-step 4th-order Runge-Kutta; f(x,t)→x_dot; returns vector (1-state) or n×T matrix |
| `lyap` | `lyap(A, Q)` | Solve A*X + X*A' + Q = 0 (Kronecker vectorization; n≤50 practical) |
| `gram` | `gram(A, B, "c")` / `gram(A, C, "o")` | Controllability or observability Gramian via lyap |
| `care` | `care(A, B, Q, R)` | Continuous Algebraic Riccati Equation → P |
| `dare` | `dare(A, B, Q, R)` | Discrete Algebraic Riccati Equation → P |
| `place` | `place(A, B, poles)` | Ackermann pole placement (SISO only) → gain vector K |
| `freqresp` | `freqresp(A, B, C, D, w)` | H(jω) at each ω; SISO → complex vector, MIMO → complex matrix |
| `nyquist` | `nyquist(G)` / `nyquist(G, w)` / `nyquist(G, "pos-only")` / `[re, im, w] = nyquist(G)` | Nyquist plot of L(jω) for a `tf` or `ss`. Auto frequency grid + two-pass densification near s = -1, conjugate mirror, -1 marker, equal aspect. Setting `axis_equal` on its panel is automatic — required for the unit circle around -1 to read as round across all four backends. |
| `sparameters` | `sparameters("amp.s2p")` / `sparameters(S, freqs)` / `sparameters(S, freqs, Z0)` | RF S-parameter network. Reads Touchstone v1.1 (`.s1p`..`.s4p`, formats `RI`/`MA`/`DB`, units `Hz`/`kHz`/`MHz`/`GHz`) or builds from a Tensor3 `[n_freqs, n_ports, n_ports]` + real freq vector (Hz). Returns a tagged `Value::Struct` with fields `parameters` (Tensor3), `frequencies` (Vector, Hz), `num_ports` (Scalar), `impedance` (Scalar Z0, default 50 Ω). Sentinel field `__kind__ = "sparameters"` triggers domain-specific `Display`. Phases 1–6 (2026-05-17) all shipped — full S-parameter toolbox per `dev/plans/closed/sparameters.md`. |
| `nports` | `nports(s)` | Port count of an sparameters network (scalar). |
| `freqs` | `freqs(s)` | Real frequency vector (Hz) of an sparameters network. |
| `sij` | `sij(s, i, j)` | Complex Vector of `S_{i,j}(f)` for 1-based port indices; errors if out of range. |
| `s11` / `s12` / `s21` / `s22` | `s11(s)` etc. | 2-port accessor sugar over `sij`; errors if `nports(s)` < required port count. Slices the parameter tensor regardless of `parameter_type` — Display tells the user the type. |
| `s2z` / `z2s` / `s2y` / `y2s` | `s2z(s)` / `z2s(z)` / `s2y(s)` / `y2s(y)` | S↔Z and S↔Y network-parameter conversions. General N-port. Per-frequency Gauss-Jordan inversion (`matrix_inv`, pure-Rust). Each conversion verifies the input `parameter_type` matches and re-tags the output. Z₀ stays scalar in Phase 2 (per-port deferred to Phase 6). |
| `s2t` / `t2s` / `s2abcd` / `abcd2s` | `s2t(s)` / `t2s(t)` / `s2abcd(s)` / `abcd2s(a)` | 2-port S↔T (cascade-form) and S↔ABCD (chain-form). Closed-form per Pozar §4.4 Tables 4.1–4.2. Hard error if input is not 2-port or if S21 ≈ 0 (T/ABCD undefined). T-parameters are used internally by `cascade` and `deembed`. |
| `cascade` | `cascade(s1, s2, ...)` | Variadic 2-port cascade via T-multiplication. Strict frequency-grid and Z₀ match (no auto-interpolation in Phase 2). Returns an S-tagged network. Requires `>= 2` arguments. |
| `deembed` | `deembed(meas, left, right)` | Recover DUT from `cascade(left, dut, right)` via `T_DUT = T_L⁻¹ · T_meas · T_R⁻¹`. All three must be 2-port S-parameters on the same frequency grid / Z₀. |
| `newref` | `newref(s, Z_new)` | Renormalise to a new reference impedance via the Z-domain detour. Updates `s.impedance` to `Z_new`. Scalar `Z_new` only in Phase 2. |
| `parameter_type` | `parameter_type(s)` | Returns the parameter-set tag as a string (`"S"`, `"Z"`, `"Y"`, `"T"`, `"ABCD"`). Default `"S"` when missing (handles structs built before Phase 2). |
| `save` (Touchstone) | `save("out.s2p", s)` | Phase 2 extension: dispatches on `.sNp` extension to `touchstone::write_touchstone`. Always emits RI format, Hz frequency unit, 15-sig-fig precision (lossless against f64). Input must be S-typed — convert with `z2s`/`y2s` first if needed. |
| `smith` | `smith(s)` / `smith(s, i, j)` / `smith(gamma)` / `smith(complex_scalar)` / `smith("file.s2p")` / `smith(..., "grid", "Z" \| "Y" \| "ZY")` | Smith chart plotting. Implementation: the grid (constant-R/-X arcs for Z, constant-G/-B for Y, both for ZY) is synthesised in `rustlab-plot::smith::build_grid` as a vector of NaN-broken polylines clipped to the unit disk. The script builtin pushes them as ordinary dashed line series with empty labels, then pushes the data trace(s), then sets `axis_equal = true` and `xlim/ylim = [-1.1, 1.1]`. Empty labels are suppressed from the legend in SVG/PNG (existing `!label.is_empty()` check), HTML (`showlegend: false`), and the viewer (no legend at all). **No backend-specific code is needed** — every rustlab backend renders the chart through its existing line-series + axis-equal path, so the live `rustlab-viewer` and animation GIF/HTML inherit Smith support for free. |
| `marker` | `marker(gamma)` / `marker(gamma, "label")` | Drop a labelled scatter point on the active Smith axes. Accepts a complex scalar or 1-element vector. Empty-string default label leaves the marker out of the legend. |
| `rfplot` | `rfplot(s)` / `rfplot(s, kind, i, j)` where `kind` ∈ `"magnitude"`, `"db"`, `"phase"`, `"unwrap"`, `"groupdelay"` | RF-engineering plots of S-parameter traces vs frequency. Default form for a 2-port renders a canonical 2×2 review panel (\|S11\|, \|S21\|, \|S12\|, \|S22\| in dB; row-major top-left to bottom-right with the standard "return loss top, forward gain top-right" layout). Falls back to a single \|S11\| dB trace for non-2-port networks. All variants route through `semilogx` so the x-axis is `log10(f)` — the canonical RF convention. Group delay τ_g = −dφ/dω uses central differences on the unwrapped phase (forward/backward at the endpoints). Phase unwrap is the standard ±2π jump rule applied to the running cumulative correction. dB values are floored at −200 dB (mag2db clamp) to keep matched ports plottable. |
| `vswr` / `return_loss` / `insertion_loss` | `vswr(s, port)` / `return_loss(s, port)` / `insertion_loss(s, i, j)` | Phase 5 port-level metrics. VSWR caps at 1e6 to avoid `∞` on full-reflect ports; return/insertion loss floor at 200 dB on matched/blocked paths. All return real `Vector` of length `n_freqs`. Math layer `eval::sparam_analysis`. |
| `gammain` / `gammaout` | `gammain(s, gamma_load)` / `gammaout(s, gamma_source)` | 2-port only. Γin = S11 + S12·S21·ΓL/(1 − S22·ΓL); Γout symmetric. Termination arg accepts a complex scalar (broadcast) or a complex `Vector` of length `n_freqs`; wrong length errors. Hard error when the denominator vanishes (load lies on the output stability boundary). |
| `stabilityk` / `stabilitymu` | `stabilityk(s)` / `[mu1, mu2] = stabilitymu(s)` | 2-port only. Rollett K and µ1/µ2 single-number stability tests. K formula `(1−\|S11\|²−\|S22\|²+\|Δ\|²)/(2·\|S12·S21\|)`. µ1 > 1 ⇔ unconditional stability (equivalent for µ2). `stabilitymu` returns a `Value::Tuple` consumed by `[a, b] = stabilitymu(s)` destructuring. |
| `gammams` / `gammaml` / `gainmax` | `gammams(s)` / `gammaml(s)` / `gainmax(s)` | 2-port only. Simultaneous-conjugate-match terminations and the resulting maximum-available-gain MAG (when K > 1) or maximum-stable-gain MSG (when K ≤ 1). Returned in dB. Quadratic-root selection in `pick_quadratic_root` picks the root with magnitude < 1 (the physically realisable termination) for `gammams`/`gammaml`. |
| `stability_circles` / `gain_circles` | `stability_circles(s, "input"\|"output")` / `gain_circles(s, gain_db)` | 2-port only. Return a tagged `Value::Struct` with `__kind__ = "stability_circles"` or `"gain_circles"`, plus fields `centres` (complex Vector), `radii` (real Vector), `frequencies` (real Vector, Hz), `domain` (`"source"` or `"load"`). For `gain_circles`, the discriminant is clamped at 0 within 1e-9 (numerical wobble at the MAG boundary collapses the circle to a point); gains genuinely beyond MAG produce NaN radii. Render via `smith_circle()` in a loop over the struct's `centres`/`radii` fields. |
| `smith_circle` | `smith_circle(centre, radius)` / `smith_circle(centre, radius, label)` | Phase 5 overlay helper. Parametric circle as a 96-vertex solid line series on the active Smith axes. Used to render `stability_circles` / `gain_circles` results one frequency at a time. Empty/missing label keeps the circle out of the legend; negative or non-finite radius errors. |
| `interp_freq` | `interp_freq(s, freqs_new)` | Phase 6: linear interpolation of complex S-parameter values onto a new monotonic frequency grid. Two-finger walk for O(n_old + n_new). Rejects extrapolation outside the source range (RF data is bandlimited; extrapolating gives worse answers than failing). Returns an S-tagged sparameters struct with the new freqs and the same Z0. |
| `noise_freqs` / `nfmin` / `gamma_opt` / `rn` / `has_noise` | `noise_freqs(s)` etc. | Phase 6: accessors for the optional 2-port noise-parameter block carried by many `.s2p` files (5 columns: freq, Fmin_dB, \|Γopt\|, ∠Γopt°, Rn/Z0). The Touchstone reader picks them up automatically when present and attaches them as `noise_frequencies`/`nf_min`/`gamma_opt`/`rn` fields on the sparameters struct. Accessors error with a clear message if the network has no noise block; `has_noise` returns a guard Bool. Writer doesn't round-trip noise data yet — `save(s, "x.s2p")` emits S-only files. |
| `s2td` | `s2td(s, i, j)` / `s2td(s, i, j, "impulse"\|"step")` | Phase 6: time-domain (step / impulse) response of a single Sij(f) trace via IFFT. Builds a 2N-point conjugate-symmetric spectrum from the user's one-sided data, IFFTs, takes the real part. Returns Tuple `[t, y]` of length 2N. Requires a uniformly spaced freq grid (rejects with a clear message otherwise — caller can use `interp_freq` first). No DC extrapolation: for spectra starting at f₀ > 0, the result is the baseband-equivalent (band-limited-pulse) response — standard VNA TDR behaviour. Cumsum integration for "step" mode. |
| `s2smm` / `smm2s` | `s2smm(s)` / `smm2s(smm)` | Phase 6: 4-port single-ended ↔ mixed-mode conversion via the standard Bockelman/Eisenstadt orthogonal transformation `M = (1/√2)·[[1,0,-1,0],[0,1,0,-1],[1,0,1,0],[0,1,0,1]]`. Forward: `Smm = M · Sse · Mᵀ`. Inverse: `Sse = Mᵀ · Smm · M`. Port pairing: ports (1, 3) → diff pair 1, (2, 4) → diff pair 2; result port order `[d1, d2, c1, c2]`. Tags transition `S ↔ Smm`. 4-port only. |
| Touchstone v2 tolerance | n/a (parser behaviour) | Phase 6: reader now accepts `[Version] 2.0` plus `[Number of Ports]`, `[Number of Frequencies]`, `[Number of Noise Frequencies]`, `[Two-Port Data Order]`, `[Matrix Format]`, `[Network Data]`, `[Noise Data]`, `[End]`, `[Begin/End Information]`. `[Reference] <scalar>` overrides the `# R` header default; per-port `[Reference]` lists and `[Mixed-Mode-Order]` tables still error clearly. |
| `svd` | `svd(A)` | SVD via symmetric eigendecomposition of A'A (real); returns Tuple [U, sigma_vector, V] where sigma is sorted descending |
| `state_init` | `state_init(n)` | Allocate FirState history buffer of length n; returns `Value::FirState` |
| `filter_stream` | `filter_stream(frame, h, state)` | Overlap-save FIR frame filter; returns Tuple `[y, state]`; history updated in-place |
| `audio_in` | `audio_in(sr, frame_size)` | Create `Value::AudioIn` descriptor (metadata only; no I/O) |
| `audio_out` | `audio_out(sr, frame_size)` | Create `Value::AudioOut` descriptor (metadata only; no I/O) |
| `audio_read` | `audio_read(src)` | Read one frame of f32 LE samples from stdin; raises `ScriptError::AudioEof` on clean EOF |
| `audio_write` | `audio_write(dst, y)` | Write real parts of frame as f32 LE to stdout; flushes after each call |
| `figure` | `figure()` / `figure("f.html")` / `figure(N)` | Create new figure (returns numeric handle) or switch to figure N; each figure has its own state and output mode (TUI/HTML/viewer) |
| `figure_live` | `figure_live(rows, cols)` | Open persistent live terminal plot; returns `Value::LiveFigure`; errors if not a tty |
| `plot_update` | `plot_update(fig, panel, y)` / `plot_update(fig, panel, x, y)` | Replace panel data (1-based panel); no immediate redraw |
| `plot_labels` | `plot_labels(fig, panel, title, xlabel, ylabel)` | Set title and axis labels on a live panel |
| `plot_limits` | `plot_limits(fig, panel, xlim, ylim)` | Set fixed axis limits on a live panel (`[lo, hi]` vectors) |
| `axis` | `axis("equal")` / `axis("auto")` / `axis([xmin, xmax, ymin, ymax])` | Lock visual aspect to 1:1 (string form) or set both axis limits at once (numeric form). String `"equal"` is honored across all four rendering backends (SVG, Plotly HTML, ratatui, viewer) — used by `nyquist` and any plot where geometric shape matters (parametric circles, complex-plane scatters). |
| `figure_draw` | `figure_draw(fig)` | Flush all panels to terminal in one atomic refresh |
| `figure_close` | `figure_close(fig)` | Drop `LiveFigure`, restoring terminal; also fires via `Drop` on script exit |
| `mag2db` | `mag2db(X)` | 20·log10(|X|) element-wise, floored at −200 dB (1e-10 floor) |
| `sparse` | `sparse(I, J, V, m, n)` / `sparse(A)` | Build sparse matrix from COO triples (1-based), or convert dense→sparse |
| `sparsevec` | `sparsevec(I, V, n)` | Build sparse vector of length n from 1-based indices and values |
| `speye` | `speye(n)` | n×n sparse identity matrix |
| `spzeros` | `spzeros(m, n)` | m×n all-zero sparse matrix |
| `full` | `full(S)` | Convert sparse to dense; identity for dense inputs |
| `nnz` | `nnz(S)` | Number of stored non-zero entries; numel for dense |
| `iscell` | `iscell(x)` | `true` if x is a string array, `false` otherwise |
| `issparse` | `issparse(x)` | 1 if sparse, 0 otherwise |
| `nonzeros` | `nonzeros(S)` | Vector of non-zero values in storage order |
| `find` | `find(v)` / `find(M)` / `[I, V] = find(v)` / `[I, J] = find(M)` / `[I, J, V] = find(M)` / sparse forms | Nargout-aware. Dense vector → 1-based element indices; dense matrix → column-major linear indices. Multi-output forms return row+col subscripts (and optionally values). Sparse vector → `[I, V]`; sparse matrix → `[I, J, V]`. Element `M(i, j)` sits at linear index `(j - 1) * nrows + i`. |
| `spsolve` | `spsolve(A, b [, mode] [, ordering])` | Solve A×x = b. `mode` is `"auto"` (default), `"cholesky"`, or `"lu"`. `ordering` is `"auto"` (default), `"identity"`/`"natural"`, or `"amd"`. Auto detects SPD (Hermitian + real-positive diagonal) and routes accordingly; auto ordering reads the matrix's `ordering_hint` (set by `laplacian_*`) and falls back to AMD when no hint is set. Identity ordering is ~5× faster than AMD on grid Laplacians. Real-vs-complex auto-detection at the entries level. Dense Value::Matrix input still uses the legacy dense Gaussian elimination. |
| `chol` | `F = chol(A [, ordering])` | Sparse Cholesky factor handle for SPD `A`. Pair with `solve(F, b)` to back-solve many RHS without re-factoring (canonical fast path for parameter sweeps and animations). Real-only `A` routes to the f64 path automatically. Errors on indefinite or non-Hermitian `A` — no auto fallback to LU. Optional `ordering` argument matches `spsolve`'s. |
| `lu` | `F = lu(A [, ordering])` | Sparse LU factor handle (partial pivoting, threshold 0.1). Same factor-once-solve-many pattern as `chol`, but works on indefinite, non-Hermitian, and complex matrices. Real-only `A` auto-routes through the f64 path. Optional `ordering` argument matches `spsolve`'s. |
| `solve` | `x = solve(F, b)` | Back-solve a right-hand side `b` through a cached factor `F` returned by `chol()` or `lu()`. Refuses a complex `b` against a real factor — refactor with the complex matrix in that case. |
| `eig` | `e = eig(A)` / `[V, D] = eig(A)` / `e = eig(A, B)` / `[V, D] = eig(A, B)` / `eig(A [, B], "vector"\|"matrix")` | Nargout-aware dense eigendecomposition. 1-output returns the `N×1` column vector of eigenvalues; 2-output returns `[V, D]` where `V` is the eigenvector matrix and `D` is a **diagonal matrix** of eigenvalues (matlab convention; `diag(D)` extracts the vector). The optional trailing string flag (matlab convention) overrides D's shape: `"vector"` forces an N×1 column, `"matrix"` forces an N×N diagonal — composes with both the standard and generalized forms. Two-arg form solves the generalized problem `A·v = λ·B·v` by reducing to standard `eig(inv(B)·A)`; requires B invertible. SPD-aware Cholesky reduction and QZ for non-invertible B are deferred. Algorithm: hand-rolled Hessenberg reduction + shifted QR for eigenvalues, then shifted inverse iteration for each eigenvector. |
| `eigs` | `[V, D] = eigs(A, n [, which])` / `[V, D] = eigs(A, B, n [, which])` | Sparse partial eigensolver. Returns the `n` smallest (`"sm"`, default) or largest (`"lm"`) eigenpairs. Standard problem `A x = λ x` or generalized `A x = λ B x` for B SPD. Auto-routes Hermitian inputs to hand-rolled Lanczos (with full reorthogonalization), general inputs to hand-rolled Arnoldi. The small dense problem is solved via Jacobi (symmetric) or shifted-QR (general). Implicit restart and shift-invert are deferred to a follow-up. |
| `loglog` | `loglog(x, y [, opts])` | Log-log line plot — x and y must be strictly positive. Implemented as a pre-transform via log10 (axes labeled "log10(x)", "log10(y)"). Same option syntax as `plot()`. |
| `semilogx` | `semilogx(x, y [, opts])` | Log-x linear-y plot. Pre-transform shim. |
| `semilogy` | `semilogy(x, y [, opts])` | Linear-x log-y plot. Pre-transform shim. |
| `polar` | `polar(theta, r [, opts])` | Polar plot via Cartesian pre-transform `(r·cos θ, r·sin θ)`. theta in radians; both real-valued. |
| `frame` | `frame()` | Snapshot the current figure into the per-thread animation frame buffer, then strip trace data from FIGURE so the next loop iteration starts clean. Subplot layout, axis labels, titles, limits, hold, and grid setting are preserved. `figure()` / `figure(N)` clears the buffer. |
| `saveanim` | `saveanim(path)` / `saveanim(path, fps)` | Flush the animation buffer to disk. Path extension picks the format: `.html` / `.htm` → self-contained Plotly animation with play/pause + slider; `.gif` → animated GIF (per-frame NeuQuant palette). `fps` defaults to 10. Errors on empty buffer or unsupported extension. Buffer is drained on success. MP4 / SVG animation deferred. |
| `spdiags` | `spdiags(V, D, m, n)` | Build sparse matrix from diagonals; D=0 main, >0 super, <0 sub |
| `sprand` | `sprand(m, n, density)` | Random sparse matrix with ~density×m×n non-zeros, values in [0,1) |
| `laplacian_1d` | `laplacian_1d(n [, dx] [, bc])` | Sparse tridiagonal Laplacian on a 1-D grid. `bc` is `"dirichlet"` (default), `"neumann"`, or `"periodic"`. |
| `laplacian_2d` | `laplacian_2d(nx, ny [, dx, dy] [, bc])` | Sparse 5-point Laplacian. Approximates `+∇²`. Column-major ordering `V(i, j) → (j-1)*ny + i`. `bc` string selects Dirichlet (default), Neumann (zero-flux; constants in null space), or Periodic (wrap; constants in null space). |
| `laplacian_3d` | `laplacian_3d(nx, ny, nz [, dx, dy, dz] [, bc])` | Sparse 7-point Laplacian on the `Tensor3` grid (axis 0 = y, axis 1 = x, axis 2 = z). Flat index `k = ((kk-1)*nx + (j-1))*ny + i`. Same `bc` semantics. |
| `laplacian_eps_2d` | `laplacian_eps_2d(eps_map [, dx, dy] [, bc])` | Variable-coefficient `∇·(ε∇)` via flux-conservative discretization with harmonic-mean half-cell coefficients. `eps_map` is `(ny, nx)` real or complex. Setting `eps_map ≡ 1` reduces to `laplacian_2d`. For magnetostatics pass `1./mu_map`. |
| `ij2k` | `ij2k(i, j, ny)` | Column-major grid → flat index. Third arg is **ny** (row count), not nx. |
| `k2ij` | `[i, j] = k2ij(k, ny)` | Inverse of `ij2k`. Third arg same caveat. |
| `ijk2k` | `ijk2k(i, j, kk, ny, nx)` | 3-D version of `ij2k`. Last two args are `ny`, `nx` (Tensor3 convention). |
| `k2ijk` | `[i, j, kk] = k2ijk(k, ny, nx)` | Inverse of `ijk2k`. |
| `sprintf` | `sprintf(fmt, args...)` | Like `fprintf` but returns the formatted string |
| `commas` | `commas(x)` / `commas(x, prec)` | Format number with thousands separators; returns Str |
| `error` | `error(msg)` | Halt script execution with a runtime error message |
| `sleep` | `sleep(seconds)` | Pause execution for a non-negative scalar duration; fractional seconds OK |
| `min` | `min(v)` / `min(M)` / `min(a, b)` / `min(M, [], dim)` / `[m, i] = min(...)` | Vector or 1-D matrix → scalar. Matrix → row of column mins (default dim 1). Two-scalar form is elementwise. 3-arg empty-placeholder form selects axis. **Multi-return** `[m, i]` available for the 1-arg vector/matrix and 3-arg axis forms; index is the 1-based first-occurrence position. Multi-return on the two-argument elementwise form errors. **Comparison key:** real value for purely-real input; magnitude `|z|` for complex input (diverges from MATLAB on equal magnitudes — rustlab uses first-occurrence, MATLAB uses phase-angle tie-break). NaN entries are skipped; all-NaN input errors. |
| `max` | `max(v)` / `max(M)` / `max(a, b)` / `max(M, [], dim)` / `[m, i] = max(...)` | Same shape and semantic rules as `min`. |
| `argmin` | `argmin(v)` / `argmin(M)` / `argmin(M, dim)` | Vector → scalar 1-based index. Matrix → row of per-column argmins (default dim 1); `dim=2` → column of per-row argmins. Comparison key and NaN/tie-break rules match `min` exactly, so `[~, i] = min(v)` and `argmin(v)` always agree. |
| `argmax` | `argmax(v)` / `argmax(M)` / `argmax(M, dim)` | Same shape and semantic rules as `argmin`. |
| `surf` | `surf(Z)` / `surf(X, Y, Z)` / `surf(X, Y, Z, cmap)` | 3D surface plot; viewer renders interactive rotate/zoom, HTML emits Plotly 3D, SVG/PNG draws a static isometric wireframe, terminal falls back to a heatmap |
| `heatmap` | `heatmap(M)` / `heatmap(M, "title")` / `heatmap(xlabels, ylabels, M [, "title" [, "colormap"]])` | Continuous-value heatmap with optional categorical axis labels. Row 0 at top. HTML emits Plotly `type:"heatmap"` with `x:`/`y:` text arrays for labels and `autorange:"reversed"` on yaxis. SVG/PNG renders cells (same path as `imagesc`) plus categorical tick formatters when both label vectors are provided. |
| `image` | `image(M)` / `image(M, "colormap")` / `image(R, G, B)` | Raw pixel display, no normalisation, values clamped to `[0, 255]`. RGB form requires three real matrices of identical shape. Stores pre-rendered RGBA on `HeatmapData` (`HeatmapKind::ImageRgba`). HTML emits Plotly `type:"image"`; SVG/PNG draws RGBA rectangles directly with no colorbar gutter. Row 0 at top. |
| `contour` | `contour(Z)` / `contour(X, Y, Z [, nlevels|levels [, "color" or "title"]])` | Line contours via marching squares. Default 10 auto-spaced round-number levels. Honours `hold on` for overlay on `imagesc`. Color spec: single-letter (k/r/g/b/c/m/y/w) or full name. Terminal: not rendered (one-time warning). HTML: Plotly contour trace. SVG/PNG: line segments. |
| `contourf` | `contourf(Z)` / `contourf(X, Y, Z [, nlevels|levels [, "title"]])` | Filled contours. Same level handling as `contour`. HTML: Plotly contour with `coloring="fill"` (exact). SVG/PNG: per-cell discrete-band approximation (exact polygon fill is HTML-only in v1). Colormap: viridis. |
| `quiver` | `quiver(X, Y, U, V [, scale or "title" or "color"])` / `quiver(U, V [, ...])` | Arrow plot of a 2-D vector field. Arrows auto-scale so the longest one equals the nearest-neighbour cell distance; user `scale` multiplies on top. NaN entries skipped. Honours `hold on` for overlay on `imagesc` / `contour`. Terminal: not rendered (one-time warning). HTML: scatter line trace with null-separated arrows. SVG/PNG: plotters line + head polyline per cell. |
| `streamplot` | `streamplot(X, Y, U, V [, density or seeds or "title" or "color"])` | Streamlines integrated via RK4 forward+backward from a seed grid (or explicit Nx2 seeds matrix), clipped at the domain boundary. NaN in `U` / `V` terminates a trace locally. Midpoint arrowhead per streamline. Same `hold on` / backend behaviour as `quiver`. Default density ≈ one seed per grid cell. |
| `gradient` | `[Fx, Fy] = gradient(F)` / `gradient(F, dx, dy)` | 2-D gradient of a scalar field on a uniform grid; rows index y, columns index x. 2nd-order central interior, 2nd-order one-sided boundaries. Each axis must have length ≥ 3. Complex inputs supported. |
| `divergence` | `divergence(Fx, Fy)` / `divergence(Fx, Fy, dx, dy)` | 2-D divergence ∂Fx/∂x + ∂Fy/∂y; Fx and Fy must share shape. Same stencils as `gradient`. |
| `curl` | `curl(Fx, Fy)` / `curl(Fx, Fy, dx, dy)` | Z-component of ∇×F (2-D scalar curl ∂Fy/∂x − ∂Fx/∂y). Same stencils as `gradient`. |
| `gradient3` | `[Fx, Fy, Fz] = gradient3(F)` / `gradient3(F, dx, dy, dz)` | 3-D gradient on a uniform grid. F is a Tensor3; axis 0 = y, axis 1 = x, axis 2 = z. Returns three Tensor3s. Same stencils and shape requirements as `gradient`. |
| `divergence3` | `divergence3(Fx, Fy, Fz)` / `divergence3(Fx, Fy, Fz, dx, dy, dz)` | 3-D divergence ∂Fx/∂x + ∂Fy/∂y + ∂Fz/∂z; all three components must share shape. Returns a Tensor3. |
| `curl3` | `[Cx, Cy, Cz] = curl3(Fx, Fy, Fz)` / `curl3(Fx, Fy, Fz, dx, dy, dz)` | 3-D curl ∇×F. Returns three Tensor3s: Cx = ∂Fz/∂y − ∂Fy/∂z, Cy = ∂Fx/∂z − ∂Fz/∂x, Cz = ∂Fy/∂x − ∂Fx/∂y. |
| `rect_mask` | `rect_mask(X, Y, x0, y0, w, h)` | Axis-aligned rectangle mask on a meshgrid. Returns an ny×nx real-valued matrix with 1.0 inside `[x0, x0+w] × [y0, y0+h]` (inclusive on all four sides) and 0.0 outside. |
| `disk_mask` | `disk_mask(X, Y, xc, yc, r)` | Closed-disk mask. Returns an ny×nx real-valued matrix with 1.0 where `(X-xc)² + (Y-yc)² ≤ r²` and 0.0 elsewhere. |
| `polygon_mask` | `polygon_mask(X, Y, verts)` | Polygon mask via even-odd ray casting. `verts` is N×2 (each row `[x, y]`); polygon is implicitly closed. Degenerate inputs (<3 vertices or all-collinear) return all-zero. |

Window names: `"hann"`, `"hamming"`, `"blackman"`, `"rectangular"`, `"kaiser"`

---

## Common Tasks

### Add a new DSP algorithm

1. Implement the function in `crates/rustlab-dsp/src/` (create a new module if needed)
2. Implement the `Filter` trait if appropriate
3. Export from `crates/rustlab-dsp/src/lib.rs`
4. Add a builtin wrapper in `crates/rustlab-script/src/eval/builtins.rs` and register it in `with_defaults()`
5. Add a CLI subcommand in `crates/rustlab-cli/src/commands/` if useful from the command line

### Add a new builtin function

1. In `crates/rustlab-script/src/eval/builtins.rs`, write:
   ```rust
   fn builtin_foo(args: Vec<Value>) -> Result<Value, ScriptError> {
       check_args("foo", &args, 1)?;
       // ... extract args with .to_scalar()/.to_cvector()/.to_str()/.to_usize()
       Ok(Value::Scalar(...))
   }
   ```
2. Register: `r.register("foo", builtin_foo);` in `with_defaults()`
3. No grammar changes needed
4. Add a `HelpEntry` in `crates/rustlab-cli/src/commands/repl.rs` and add the name to the appropriate `CategoryRow` in `CATEGORIES` (pick one of the 12 toolboxes: `language`, `math`, `linalg`, `stats`, `sparse`, `dsp`, `spectral`, `controls`, `rf`, `pde`, `plot`, `audio`) — required, not optional (see Workflow Rule 3). The `help_coverage_tests` unit tests fail if you skip this.
5. Add the function to `docs/functions.md` with its signature, description, and an example (see Workflow Rule 5)
6. Write at least one unit test in `crates/rustlab-script/src/tests.rs` (see Workflow Rule 2)

### Add a new `Value` type

1. Add variant to `Value` enum in `src/eval/value.rs`
2. Add `negate`, `binop`, `Display` match arms
3. Add `to_*` conversion method if needed
4. Update `from_matrix_rows` if the type can appear in `[...]` literals

### Add matrix decompositions (future)

1. Create `crates/rustlab-linalg/` depending on `rustlab-core` with `linalg` feature
2. Implement `Decomposable` + the appropriate marker trait on `CMatrix`
3. Enable feature in workspace: `rustlab-core = { ..., features = ["linalg"] }`

---

## Design Decisions

| Decision | Rationale |
|---|---|
| All numbers are `Complex<f64>` | Avoids type promotion complexity; real signals just have `im = 0` |
| `j` is a constant not a syntax token | Keeps the lexer simple; `j*x` works naturally through arithmetic |
| 1-based indexing | Consistent with signal processing convention |
| Trailing `;` suppresses output | Familiar to anyone who has used a scientific computing language |
| `BuiltinRegistry` is a `HashMap` | Adding a function never requires touching the parser or grammar |
| `Decomposable` stubs exist now | Ensures the trait boundary is defined before any implementors are written |
| `ratatui` for plotting | Braille-pixel rendering in the terminal; alternate screen leaves no scrollback artifacts |
| `rustyline` for REPL | Provides readline history and line editing with minimal code |
| No `todo!()` stubs in production code | All implemented paths are complete; unimplemented paths return `CoreError::NotImplemented` |

---

## Error Handling Conventions

- `rustlab-core` → `CoreError`
- `rustlab-dsp` → `DspError` (wraps `CoreError` via `#[from]`)
- `rustlab-plot` → `PlotError`
- `rustlab-script` → `ScriptError` (wraps `CoreError`, `DspError`, `PlotError` via `#[from]`)
- `rustlab-cli` → `anyhow::Error` (converts all library errors at the boundary)

Use `?` to propagate. Do not panic except in `unreachable!()` for truly impossible arms.

**Special case — `ScriptError::AudioEof`:** Raised by `audio_read` when stdin closes cleanly mid-frame (the upstream producer finished). `rustlab-cli/src/commands/run.rs` intercepts this variant and maps it to `Ok(())` (exit code 0, no error message). It is never printed to the user — it is the normal end-of-stream signal for streaming pipelines.

---

## How to Add Tests

Tests are **required** for every new feature (see Workflow Rules above). Run the full suite with:

```sh
cargo test --workspace
```

### DSP algorithm tests — `crates/rustlab-dsp/src/tests.rs`

Test concrete mathematical properties:

```rust
#[test]
fn lowpass_coefficients_sum_to_one() {
    // A lowpass FIR with rectangular window has DC gain = 1
    let f = fir_lowpass(31, 0.25 * 44100.0, 44100.0, WindowFunction::Rectangular).unwrap();
    let sum: f64 = f.coefficients.iter().map(|c| c.re).sum();
    assert!((sum - 1.0).abs() < 1e-6, "DC gain was {sum}");
}

#[test]
fn convolution_with_delta_is_identity() {
    let x = Array1::from_vec(vec![1.0, 2.0, 3.0]);
    let delta = Array1::from_vec(vec![1.0]);
    let y = convolve(&x, &delta);
    assert_eq!(y.len(), x.len());
    for (a, b) in x.iter().zip(y.iter()) { assert!((a - b).abs() < 1e-12); }
}
```

### Interpreter / builtin tests — `crates/rustlab-script/src/tests.rs`

Use `run()` to evaluate snippets and inspect the returned environment:

```rust
#[test]
fn inv_times_a_is_identity() {
    let src = "A = [1,2;3,4]; B = inv(A) * A";
    let mut ev = Evaluator::new();
    ev.run(src).unwrap();
    // B should be approximately the 2×2 identity
    if let Value::Matrix(m) = ev.get("B").unwrap() {
        assert!((m[[0,0]].re - 1.0).abs() < 1e-10);
        assert!((m[[0,1]].re).abs() < 1e-10);
    } else { panic!("expected Matrix"); }
}
```

### Integration tests — `crates/rustlab-cli/tests/examples.rs`

Run example scripts and assert they exit cleanly:

```rust
#[test]
fn example_lowpass_runs() {
    let status = Command::new(env!("CARGO_BIN_EXE_rustlab"))
        .args(["run", "examples/dsp/lowpass.rlab"])
        .status().unwrap();
    assert!(status.success());
}
```
