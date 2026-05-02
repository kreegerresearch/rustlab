# Migration: `.r` → `.rlab` (rustlab script extension)

**Status:** Plan, awaiting user review. Implementation work was started and is stashed (`git stash` entry: "rename-r-to-rlab WIP: 70 git mv + source edits + run_compare.sh + perf/run_perf.sh + partial README"). Do NOT pop the stash until this plan is approved.
**Branch:** `rename-r-to-rlab`
**Date opened:** 2026-05-02

## 1. Strategic context

**rustlab is its own language.** It is a domain-specific language for DSP and matrix modeling — not a flavour of R, not an Octave/MATLAB script, and not bound to follow any other language's conventions. The `.r` extension was a stopgap that GitHub mis-identifies as the R statistics language (wrong syntax highlighting, wrong language-bar accounting, wrong tooling defaults).

This migration cuts that confusion at the file extension level and establishes the foundation for rustlab's own language identity. Until a native rustlab grammar and GitHub Linguist definition are published, **MATLAB syntax highlighting is used as a temporary proxy** — and only as a proxy. rustlab does not always follow octave/matlab convention (the `j`-only imaginary unit, the rustlab-specific REPL builtins like `viewer on`, the notebook subsystem, the `frame()`/`saveanim()` animation API, etc. are rustlab-native), and the documentation must be explicit about this so readers don't infer that any matlab idiom is supported just because the highlighter colours it.

Every doc artifact this PR adds or touches must reinforce that point — README §4 intro, the `.gitattributes` comment, the GitHub language-bar note, and the editor-setup snippets all need a phrase calling out that the matlab association is for highlighting only.

The CLI (`rustlab run path`) and the script-side `run path` keyword are extension-agnostic — the lexer/parser don't parse `.r` specifically. So the migration is mechanical: rename files, update references, drop a `.gitattributes` hint, document editor setup.

## 2. File rename operations

`git mv` so the renames track as renames, not delete+add. **70 files** total (one more than the 63 from the original survey — `perf/bench_*.r` was missed initially):

| Location | Count | Notes |
|---|---|---|
| `examples/*.r` | 56 | top-level examples |
| `examples/audio/*.r` | 3 | interactive audio scripts |
| `examples/controls/*.r` | 16 | controls topics |
| `examples/sparse/sparse.r` | 1 | sparse demo |
| `examples/tensor3/tensor3.r` | 1 | rank-3 tensor demo |
| `tests/octave/rustlab_outputs.r` | 1 | rustlab side of compare suite |
| `tests/octave/rustlab_full.r` | 1 | rustlab side of full compare suite |
| `perf/bench_*.r` | 7 | hand-rolled benchmark scripts |

Verification command after the renames: `find . -name "*.r" -not -path "./target/*" -not -path "./.git/*"` should return zero results.

## 3. GitHub configuration

Add `.gitattributes` at the repo root:

```
# Use MATLAB highlighting as a temporary proxy for the rustlab language
# until a native rustlab Linguist definition is published.
*.rlab linguist-language=MATLAB
```

This pins rustlab files at the GitHub language statistics bar to "MATLAB" rather than letting them fall through to plain text or get auto-detected as something unrelated. The doc note in section 4 makes the temporary nature explicit so anyone landing on the project page understands why it shows up as MATLAB.

## 4. README documentation

Add a new "Environment & Tooling" section to `README.md` (placement: after the existing language overview / quickstart, before the example tables). Three sub-sections:

### 4.1 The `.rlab` language extension

> rustlab uses the `.rlab` extension for its DSP modeling files. While rustlab is a distinct language, we currently leverage MATLAB/Octave syntax highlighting definitions as a temporary measure for visual clarity in development environments — a native rustlab grammar is on the roadmap. The "MATLAB" label on this repo's GitHub language bar reflects that proxy mapping; the actual language is rustlab.

### 4.2 Visual Studio Code

```jsonc
// settings.json
"files.associations": {
    "*.rlab": "matlab"
}
```

### 4.3 Neovim / Vim

```lua
-- Neovim init.lua
vim.filetype.add({
  extension = {
    rlab = 'matlab',
  },
})
```

```vim
" Vim equivalent
autocmd BufRead,BufNewFile *.rlab setfiletype matlab
```

### 4.4 GitHub language detection

> Repository highlighting is managed via `.gitattributes` (`*.rlab linguist-language=MATLAB`). GitHub's language statistics bar shows the project as "MATLAB" — this is intentional and temporary, until a native rustlab Linguist definition lands.

## 5. Interpreter log identification

Per the user's prompt: "the rustlab compiler/interpreter explicitly identifies itself in logs as the handler for .rlab files to reinforce the language's independent identity."

**Implementation** (`crates/rustlab-cli/src/commands/run.rs`):

When `rustlab run <path>` is invoked, before evaluation begins, print to stderr:

```
rustlab N.N.N — interpreting <path> (.rlab)
```

A single `eprintln!` at the top of `commands/run::execute`. Cheap, unambiguous, only fires when the user explicitly invokes the run subcommand. **Always-on**, no flag gating — the goal is rustlab actively naming itself as the handler. Future opt-out via `--quiet` is a five-line follow-on if the noise ever becomes a complaint; not in this PR.

The REPL stays quiet (different code path), the integration tests in `crates/rustlab-cli/tests/examples.rs` don't break (they check status / stdout, not stderr), and library callers using `rustlab_script::run(...)` from Rust are unaffected (no CLI involvement).

## 6. Source-code reference updates

These files mention `.r` in user-facing strings or doc comments. All shown updates are text-only — no behavior change.

| File | What's there | Change |
|---|---|---|
| `crates/rustlab-script/src/lib.rs` | 3 doc comments mentioning `.r` | `.rlab` |
| `crates/rustlab-script/src/ast.rs` | 1 doc comment for `run` statement | `.rlab` |
| `crates/rustlab-cli/src/cli.rs` | `about = "...scriptable .r language"`, `Execute a .r script file` | `.rlab` |
| `crates/rustlab-cli/src/commands/info.rs` | `Scripting: rustlab run script.r` | `script.rlab` |
| `crates/rustlab-cli/src/commands/repl.rs` | HelpEntry rows for `run` and `profile` | `.rlab` |
| **`crates/rustlab-cli/tests/examples.rs:29,90`** | hardcoded `format!("{name}.r")` and `"fixed_point.r"` | **must change** in same commit — affects test |
| `crates/rustlab-plot/src/viewer_live.rs` | 1 comment mentioning `surf.r` | `surf.rlab` |
| `crates/rustlab-cli/src/commands/run.rs` | new: log line per §5 above | add |

## 7. Build / test scripts

- `tests/octave/run_compare.sh` — `"$RUSTLAB" run rustlab_outputs.r` and `rustlab_full.r` (2 hardcoded names) → `.rlab`
- `perf/run_perf.sh` — `for f in "$PERF_DIR"/bench_*.r` and `name=$(basename "$script" .r)` (3 hardcoded references) → `.rlab`

## 8. Documentation sweep

- `README.md` — ~50 references in tables and prose (the new sections in §4 also land here).
- `AGENTS.md` — references to `.r files`, `examples/lowpass.r`, etc.
- `docs/examples.md` — per-example reference page (~30 mentions).
- `docs/validate_octave_report.md` — `rustlab_outputs.r` reference.
- `docs/quickref.md` — quick scan; minor mentions.
- `llms.txt` — `Script extension: .r` line.
- `examples/notebooks/animation.md`, `examples/notebooks/laplacian_bc.md` — embed `examples/*.r` paths in prose.
- `gallery/` — regenerated by `make notebooks`; no manual edits.
- `dev/plans/sparse.md`, `dev/plans/octave_compat_divergences.md`, `dev/plans/profiling.md`, `dev/plans/tensor3.md`, `perf/performance.md` — historical plans that reference example/bench paths. Sweep so future readers don't follow dead links.

## 9. Order of operations

Single coordinated commit on the `rename-r-to-rlab` branch — all renames, source-code references, `tests/octave/run_compare.sh`, `perf/run_perf.sh`, the new README section, the log line, `.gitattributes`, and the **0.1.12 → 0.1.13 version bump** land together.

1. **Rename files with `git mv`** (§2) so git tracks as renames.
2. **Update source-code references** (§6) — including the new log line in `commands/run.rs`.
3. **Update `tests/octave/run_compare.sh`** and **`perf/run_perf.sh`** (§7).
4. **Add `.gitattributes`** (§3) — new file, two lines.
5. **Update `README.md`** — bulk-rewrite example paths (perl works better than BSD sed for the boundary regex), then hand-write the new "Environment & Tooling" section (§4).
6. **Update remaining docs** (§8) — `AGENTS.md`, `docs/examples.md`, `docs/validate_octave_report.md`, `docs/quickref.md`, `llms.txt`, the two notebook prose files, the historical plans.
7. **Regenerate gallery** via `make notebooks` so any references inside notebook prose flow through.
8. **Validate** (§10).
9. **Commit + push.**

## 10. Verification checklist

- [ ] `find . -name "*.r" -not -path "./target/*" -not -path "./.git/*"` returns zero rustlab-script files.
- [ ] `cargo build --workspace` clean.
- [ ] `cargo test --workspace` green (currently 1,388 tests; the integration test in `crates/rustlab-cli/tests/examples.rs` is the most important regression target — it actually loads the renamed paths, so a missed rename would surface here).
- [ ] `bash tests/octave/run_compare.sh` green (148 cases at machine precision).
- [ ] `bash perf/run_perf.sh` green (smoke; not run in CI but should still execute).
- [ ] `make notebooks` clean — only `seed.md`'s deliberate drift in the diff.
- [ ] `./target/release/rustlab run examples/eig.rlab` produces the expected log line `rustlab N.N.N — interpreting examples/eig.rlab (.rlab)` followed by clean output.
- [ ] `grep -rn "\.r\b" --include="*.md" --exclude-dir=target --exclude-dir=.git --exclude-dir=gallery` shows no remaining rustlab-script paths in markdown docs.
- [ ] `.gitattributes` exists at repo root with the documented entry.
- [ ] README has the "Environment & Tooling" section with all three editor configurations.

## 11. Decisions confirmed with user

1. **Scope:** rename `examples/`, `tests/octave/rustlab_*.r`, AND `perf/bench_*.r`. Project-wide convention.
2. **Backward compat:** hard cut — no `.r` symlinks. Pre-1.0 project; clean diff.
3. **`.gitattributes`:** add `*.rlab linguist-language=MATLAB` per user's revised prompt.
4. **Single commit** on the `rename-r-to-rlab` branch, all changes together.
5. **Always-on log banner** — no flag gate, no `--quiet` in this PR.
6. **Historical `dev/plans/*.md` sweep:** rewrite example/bench path references to `.rlab` so future readers don't follow dead links.
7. **Version bump:** `0.1.12` → `0.1.13` in `Cargo.toml` (workspace + 6 path-dep entries; per-crate `Cargo.toml` files use `version.workspace = true`). Lands in the same commit. Already done in this branch's working tree.

## 12. Risks

- **Stray hardcoded path** missed in the survey — caught by the integration test in `crates/rustlab-cli/tests/examples.rs` and the octave compare suite.
- **Stash collision** — the in-flight work is currently stashed. If the user wants any part rolled forward, `git stash pop` will reapply (but the stash content reflects the *previous* plan that used `linguist-language=Octave`). Need to either pop and amend, or drop the stash and redo from this plan.
- **External users** with bookmarked paths or scripts that source `examples/<name>.r` — clean cut breaks for them. Pre-1.0; flag in release notes.

## 13. Next step

User reviews this plan. On approval, the implementation walk-through is straightforward: pop the stash, replay against this plan's spec (especially the `.gitattributes` linguist-language change to MATLAB and the new README section + log line which weren't in the stashed work yet), then run the verification checklist.

Alternative: drop the stash and start fresh — likely cleaner since the §4 README spec and §5 log line aren't in the stash.
