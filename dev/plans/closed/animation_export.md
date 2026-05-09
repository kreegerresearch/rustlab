# Implementation Plan — Multi-Frame Animation Export

**Status:** Phases 1–3 + GIF shipped on `feat/animation-export`. Phase 4 shipped on rustlab_em `feat/animation-export-landed`.
**Date opened:** 2026-04-26
**Source request:** `../rustlab_em/dev/rustlab/requests/animation-export.md` — was the only one of `rustlab_em`'s original five upstream requests still **Proposed**; now Landed.
**Scope:** Plotly-HTML animation (Option A) **and** animated GIF (Option B) shipped together. MP4 / animated SVG / APNG remain out of scope.

This plan turns the request into a sized, sequenced work program against the rustlab tree at `/Users/mike/projects/2026/rustlab`. It does **not** introduce a new dependency: Plotly's animation runtime is already loaded by every HTML figure we emit. We just need to produce a richer JSON document and add two builtins that drive it.

## Why now / what it unblocks

`rustlab_em` Lessons 08 (Maxwell's equations), 09 (plane waves), and 11 (FDTD) all have time as a first-class variable. A static `imagesc` SVG can show *one* frame of a propagating wave; the whole story (incidence, reflection, refraction, scattering, dispersion) only reads as a movie. Today those lessons are stubs (~30 lines each). Shipping `frame()` / `saveanim()` lets the curriculum draft those lessons against a real builtin instead of the manual `savefig("frame_%04d.svg" % t)` + `ffmpeg` workaround.

No code is currently broken without this — it is a feature-add, not a bug-fix.

## Out of scope (explicitly)

- **GIF / MP4 export.** The request lists this as "Option B"; we are not building it in this plan. The deferral is intentional: GIF requires a per-frame raster pass through `BitMapBackend` (slow — seconds per frame) and a new pure-Rust dependency (`gif` crate). If a concrete need emerges later (PDF embedding, social-media share), layer it on top of the same `frame()` buffer this plan introduces — the dispatch happens at flush time based on path extension. Documented as a follow-up at the bottom of this plan.
- **Animated SVG / animated PNG (APNG).** Our `plotters` SVG path is single-shot and there is no notebook-side use case for animated SVG. APNG is browser-spotty.
- **Interactive 3-D animation (`surf` time series).** Plotly's 3-D scenes do animate, but emit-side complexity is much higher (scene-camera state per frame). Phase 4 carves out a smoke-test for `surf` animation but does not commit to shipping it; if it falls out of the existing Plotly scene emitter for free, ship it; otherwise punt to a follow-up plan.

## Licensing policy

No new dependency. Plotly.js is already loaded from CDN by every HTML figure (`<script src="https://cdn.plot.ly/plotly-2.35.0.min.js">` at `crates/rustlab-plot/src/html.rs:75`). The animation runtime is part of that bundle — no new crate, no new feature flag, no new licensing review. AGENTS.md Rule 9 (core algorithms in pure Rust) is unaffected: this is rendering, not numerics.

## Workflow obligations (apply per phase, not repeated below)

Six mandatory rules per `feedback_workflow.md` and `AGENTS.md:165-173`:
1. **Plan first** — this document, approved before code.
2. **Tests in same commit** — unit tests in `crates/rustlab-plot/src/animation.rs`; builtin tests in `crates/rustlab-script/src/tests.rs`. Run `cargo test --workspace` *and* `cargo test --workspace --features viewer`.
3. **No commit without explicit approval.**
4. **Update `AGENTS.md`** — function table (~lines 817-925).
5. **Update `docs/QuickRef.md`** — under a new "Animation" subsection or under "Plotting".
6. **Update `docs/functions.md` + REPL `HelpEntry` + category list** in `crates/rustlab-cli/src/commands/repl.rs`.

## Architectural facts the plan rests on

- The thread-local `FIGURE` (`crates/rustlab-plot/src/figure.rs:279`) holds a `FigureState` (`figure.rs:226`). Every plot builtin (`plot`, `imagesc`, `quiver`, `contour`, etc.) mutates `FIGURE` in place.
- HTML output: `render_figure_html(path)` → `render_figure_state_html_themed` → `render_figure_plotly_div(fig, "plot", theme)` (`html.rs:48, 56, 61, 99`). The div emitter writes a single `Plotly.newPlot("plot", traces, layout)` call.
- Plotly's animation API is `Plotly.newPlot(div, data, layout, { frames: [...] })` plus `updatemenus` for play/pause and a `sliders` config for the scrubber. Each frame is `{ name: "0", data: [{ z: ... }, { x: ... }] }` — same trace ordering as the base `data` array, only the fields that *change* need to be present.
- `figure_new()` (`figure.rs:431`) replaces `FIGURE` wholesale; `figure_switch(id)` swaps to a stored `FigureState` by id.
- `savefig(path)` dispatches on extension via `render_figure_file(path)` (`file.rs:107`): `.html`/`.htm` → `render_figure_html`, otherwise → `plotters` SVG/PNG.
- `FigureState: Clone` is already implemented (`figure.rs:225` derive). Snapshotting is a single `.clone()`.
- Notebook pipeline (`make notebooks` / `dev/build-notebooks.sh`) renders to `gallery/<name>.html`. Animations drop into that pipeline with no extra glue once `saveanim` honours `.html` paths.
- REPL help: `crates/rustlab-cli/src/commands/repl.rs` holds `HelpEntry` records (~lines 13-510) and the `categories` table (~lines 813-1002). Two new entries (`frame`, `saveanim`) plus inclusion in a "Plotting / animation" category line.

## Public API (proposed)

Two new builtins, no changes to existing ones.

```rustlab
% Build a 60-frame animation of a propagating wave
figure()                        % clears FRAMES buffer + FIGURE
for t = 0:dt:T
  Ez = solve_step(t);
  imagesc(Ez, "viridis")
  title(sprintf("t = %.2f ns", t*1e9))
  frame()                       % snapshot FIGURE → FRAMES, clear traces (keep axes/labels)
end
saveanim("wave.html", 30)       % flush FRAMES as Plotly animation @ 30 fps
```

| Builtin | Signature | Semantics |
|---|---|---|
| `frame()` | no args, returns `Value::None` | Clones the current `FIGURE` into the per-thread `FRAMES` buffer. Then clears the *trace data* on the current figure (`series`, `heatmap`, `surface`, `contours`, `quivers`, `streamlines` on every subplot) so the next iteration starts fresh, but keeps subplot layout, axis labels, titles, xlim/ylim, hold state, and grid setting. |
| `saveanim(path)` / `saveanim(path, fps)` | `path: String`, optional `fps: f64` (default 10) | Flushes the `FRAMES` buffer to disk as a Plotly HTML animation. `fps` becomes per-frame duration in ms (`1000/fps`). Errors if `FRAMES` is empty or path does not end in `.html`/`.htm`. After flush, **clears** `FRAMES` so a subsequent `frame()` loop starts clean without an explicit `figure()`. |

`figure()` semantics extended: it clears the `FRAMES` buffer (in addition to its existing reset of `FIGURE`). This makes "start a new animation" the natural pattern.

### Edge cases

| Case | Behaviour |
|---|---|
| `saveanim(...)` with empty `FRAMES` | Runtime error: `saveanim: no frames captured (call frame() at least once)` |
| `saveanim("foo.svg")` or any non-HTML path | Runtime error: `saveanim: only .html / .htm output is supported in this release` (clear path forward to GIF later). |
| `frame()` called without any plot calls in between | Snapshot anyway — produces an "empty" frame. Cheap, predictable, no special-case. |
| `frame()` called while `hold` is on | Same as `hold off` flow: snapshot, then clear traces but leave `hold = on` so the next iteration accumulates correctly. |
| `figure(N)` (switch) during a frame loop | Permitted — `FRAMES` is one buffer per thread, not per figure. Snapshots whichever figure is active when `frame()` fires. Documented; no extra machinery. |
| Mixed subplot counts across frames | First frame's `subplot_rows × subplot_cols` is canonical. Later frames with different counts: error at flush time, since Plotly cannot animate variable subplot grids cleanly. |
| Mixed plot kinds across frames (e.g. `imagesc` then `plot`) | Allowed within a panel — Plotly accepts trace replacement. We emit each frame's full trace set (not delta-only) to keep the runtime simple. |

## Memory budget

`FigureState` carries every series's `Vec<f64>` data inline. For an FDTD demo on a 200×200 grid with 500 frames:
- Heatmap `z`: 200·200·8 B = 320 KB per frame.
- 500 frames ≈ 160 MB resident before flush. Plotly handles it; the bottleneck is browser JS heap on load, not our buffer.

For curricular use (typical 100×100 grid, ~120 frames) we are well under 50 MB resident. Document the back-of-the-envelope in `docs/functions.md`. No streaming-flush mode in this release; revisit if a lesson actually breaks the budget.

## Phase plan

Each phase is a single commit with tests + docs. Phases land sequentially; no parallel branches.

### Phase 1 — Frame buffer + `frame()` builtin

**What lands**
- New module `crates/rustlab-plot/src/animation.rs` with:
  - `thread_local! { static FRAMES: RefCell<Vec<FigureState>> = ...; }`
  - `pub fn push_frame()` — clones `FIGURE` into `FRAMES`.
  - `pub fn clear_frames()` — empties the buffer.
  - `pub fn frames_len() -> usize`.
  - `pub fn take_frames() -> Vec<FigureState>` — drain for renderer.
  - `pub fn clear_figure_traces()` — wipes `series` / `heatmap` / `surface` / `contours` / `quivers` / `streamlines` on every `SubplotState` of the current `FIGURE` while preserving layout, labels, limits, hold, grid.
- Wire into `crates/rustlab-plot/src/lib.rs` re-exports.
- Extend `figure_new()` (`figure.rs:431`) and `figure_switch(id)` to call `clear_frames()` on entry. (Keep this minimal — one line each.)
- New `frame()` builtin in `crates/rustlab-script/src/eval/builtins.rs`:
  ```rust
  fn builtin_frame(args: Vec<Value>) -> Result<Value, ScriptError> {
      check_args("frame", &args, 0)?;
      rustlab_plot::push_frame();
      rustlab_plot::clear_figure_traces();
      Ok(Value::None)
  }
  ```
  Register in `register_builtins`.

**Tests** (in `crates/rustlab-plot/src/animation.rs` `#[cfg(test)] mod tests`)
- `push_frame_increments_buffer` — call `imagesc`-like setup, `push_frame()`, assert `frames_len() == 1`.
- `clear_figure_traces_keeps_layout` — set title, xlim, hold; clear; assert title/xlim/hold survived but `series` is empty.
- `figure_new_clears_frames` — push 3 frames, call `figure_new()`, assert `frames_len() == 0`.
- `take_frames_drains` — push 2; `take_frames().len() == 2`; `frames_len() == 0` after.

**Tests** (in `crates/rustlab-script/src/tests.rs`)
- `frame_builtin_snapshots_imagesc` — script: `imagesc([1,2;3,4]); frame(); imagesc([5,6;7,8]); frame();` then assert `FRAMES` length 2 and each frame's heatmap matrix differs.
- `frame_clears_traces_for_next_step` — assert second `imagesc` does not see leftover series from first.

**Docs in this commit**
- `docs/functions.md` — new `### frame()` entry under a new "## Animation" section.
- `docs/QuickRef.md` — one row in the plotting block.
- `crates/rustlab-cli/src/commands/repl.rs` — `HelpEntry { name: "frame", brief: "Snapshot current figure into the animation frame buffer", detail: ... }` plus inclusion in a "Plotting" category list line.
- `AGENTS.md` function table — `frame()` row.

**Out of scope this phase**
- `saveanim()` — Phase 2 (separate commit).

**Acceptance**
- `cargo test --workspace --features viewer` green.
- `rustlab` REPL `help frame` returns the new entry.
- No HTML output yet — frames are buffered, that's it.

### Phase 2 — `saveanim()` + Plotly HTML animation emitter

**What lands**
- Extend `crates/rustlab-plot/src/animation.rs`:
  - `pub fn render_animation_html(path: &str, fps: f64) -> Result<(), PlotError>`
  - Drains `FRAMES` (`take_frames()`).
  - First frame becomes the base trace set + layout — re-uses `render_figure_plotly_div(&frames[0], "plot", theme)` to keep colour/theme parity with static HTML.
  - Subsequent frames serialise to a Plotly `frames: [...]` JSON array. Frame `name` = string of zero-padded index. Each frame emits the *same* trace ordering as the base; only fields that meaningfully vary across frames (`z` for heatmaps, `x`/`y` for line series, `u`/`v` for quivers) need to be present, but for v1 we emit the full trace per frame to keep the renderer simple. (Optimise later if memory becomes the bottleneck.)
  - Inject Plotly `updatemenus` (play/pause buttons) and `sliders` (per-frame scrubber) into the `layout` object.
  - Frame duration = `1000/fps` ms; transition duration = `0` (hard cuts; smoother transitions look wrong for FDTD-style data).
- New `saveanim` builtin in `builtins.rs`:
  ```rust
  fn builtin_saveanim(args: Vec<Value>) -> Result<Value, ScriptError> {
      check_args_range("saveanim", &args, 1, 2)?;
      let path = args[0].to_str()?;
      let fps = if args.len() == 2 { args[1].to_scalar()? } else { 10.0 };
      if !(path.ends_with(".html") || path.ends_with(".htm")) {
          return Err(ScriptError::runtime(
              "saveanim: only .html / .htm output is supported".into()
          ));
      }
      if rustlab_plot::frames_len() == 0 {
          return Err(ScriptError::runtime(
              "saveanim: no frames captured (call frame() at least once)".into()
          ));
      }
      rustlab_plot::render_animation_html(&path, fps)
          .map_err(|e| ScriptError::runtime(e.to_string()))?;
      Ok(Value::None)
  }
  ```

**Tests** (in `crates/rustlab-plot/src/animation.rs`)
- `render_animation_html_writes_file_with_frames_block` — push 3 frames, render to `tempfile.html`, read back, assert the file contains:
  - `Plotly.newPlot(`
  - `frames: [` (the array marker)
  - exactly 3 `name: "` occurrences (one per frame).
- `render_animation_html_includes_play_button` — assert `updatemenus` and `Play` substrings present.
- `render_animation_html_errors_on_empty_buffer` — call without pushing → error.

**Tests** (in `crates/rustlab-script/src/tests.rs`)
- `saveanim_round_trip` — run the full `for ... frame() ... end; saveanim("/tmp/anim.html", 30)` script via the existing test harness; assert file exists and contains `frames:` block.
- `saveanim_rejects_svg` — `saveanim("foo.svg")` returns the documented error.
- `saveanim_clears_buffer` — after success, `frames_len() == 0`.

**Docs in this commit**
- `docs/functions.md` — `### saveanim(path)` / `### saveanim(path, fps)` with the canonical FDTD-style example.
- `docs/QuickRef.md` — one row.
- REPL `HelpEntry` for `saveanim` with detail string.
- `AGENTS.md` function table.

**Acceptance**
- A hand-written `examples/animation_smoke.rlab` (kept short, not yet a polished example) renders to a viewable `gallery/animation_smoke.html` with a play bar and slider.

### Phase 3 — Example + notebook + gallery integration

**What lands**
- `examples/animation_wave.rlab` — small standalone (~30 lines): seed RNG, build a 60-frame travelling Gaussian pulse on a 100×100 grid, save `gallery/animation_wave.html`. No FDTD physics yet — that belongs in `rustlab_em`. Just enough to demonstrate the API and produce a shareable artefact.
- `examples/notebooks/animation.md` — the lesson-style notebook version of the example, with prose explaining `frame()` / `saveanim()` semantics, the memory budget, and limitations (HTML-only in this release).
- `gallery/animation.html` (rendered by `make notebooks`).
- Update `examples/notebooks/README.md` and `gallery/README.md` to list the new notebook.

**Tests**
- No new unit tests this phase. Notebook and example are the smoke test; CI's `make notebooks` step will fail loudly if the script does not run cleanly.

**Acceptance**
- `make notebooks` runs to completion.
- Open `gallery/animation.html` in a browser → play button works, slider scrubs through 60 frames, no console errors.

### Phase 4 — Curriculum integration (rustlab_em side, separate PR)

**What lands** (in `rustlab_em`, not `rustlab`)
- Update Lesson 09 (`notebooks/09-em-waves.md`) to use `frame()` / `saveanim()` for the propagating-wave demo it currently sketches.
- Update Lesson 11 (`notebooks/11-fdtd-simulation.md`) similarly for the canonical FDTD movie.
- Flip `rustlab_em/dev/rustlab/requests/animation-export.md` Status from **Proposed** to **Landed**.
- Update the table in `rustlab_em/dev/rustlab/requests/README.md:9-15` so the animation row reads **Landed**.
- (Cross-repo PR; this plan tracks it but the work happens in the curriculum repo.)

**Acceptance**
- Lessons 09 and 11 render cleanly under the rustlab_em build pipeline.
- The lesson HTML output contains a working animation.

**Stretch goal (only if it falls out for free):** a `surf` animation smoke test in the same notebook. If `render_figure_plotly_div`'s scene-emission code already round-trips per-frame, it costs ~5 lines to ship; if not, defer.

## Risk register

| Risk | Likelihood | Mitigation |
|---|---|---|
| Plotly's per-frame JSON balloons the HTML beyond browsers' practical limits (~50 MB) | Medium for FDTD-scale grids | v1 emits full trace per frame for simplicity. If users hit this, switch to delta-only frame emission (only `z` updates) — a Phase 2.5 follow-up, well-scoped. |
| `clear_figure_traces` semantics surprise users (e.g. they expected `frame()` to also reset `title`) | Low | Document the *exact* preservation list in `docs/functions.md`. Lean on convention from matplotlib's `FuncAnimation` (which similarly keeps axes between frames). |
| Plotly version pinned to 2.35.0 in `html.rs:75` lacks an animation feature we depend on | Very low (2.35.0 has animations since v1) | The full Plotly animation API has been stable since 2018. No version bump needed. |
| Notebook pipeline does not pick up `saveanim` output because the renderer assumes single-output-per-cell | Low | Audit `dev/build-notebooks.sh` in Phase 3 before writing the notebook. If it filters on `savefig`, extend to recognise `saveanim`. |
| `take_frames()` interacts poorly with figure-id switching (`figure(2); ... figure(1); saveanim(...)`) | Low | `FRAMES` is one buffer per thread, not per figure. Document this in the `saveanim` detail string. If lessons demand per-figure buffers, redesign in a follow-up — out of scope for v1. |

## Open questions for review

1. **Should `figure()` clear `FRAMES`, or should we add an explicit `clearframes()`?** Plan currently lumps the clear into `figure()`. Argument *for*: matches matplotlib's "new figure → new animation" muscle memory. Argument *against*: a power user might want to switch figures mid-animation without losing buffered frames. Default position: clear in `figure()`. Easy to revisit.
2. **Default fps**: 10 (matches the canonical Plotly tutorial), 30 (smooth video), or 60 (overkill for FDTD)? Plan: 10. Noted in case there's a strong preference.
3. **Plotly transition mode**: hard cut (`transition: { duration: 0 }`) or 100 ms ease-in-out? Plan: hard cut. Crossfading heatmap z-values produces visually wrong intermediate states for physical fields.
4. **Should `frame()` preserve `xlim`/`ylim` set on the *first* frame across the whole animation?** Today each frame can have its own limits, which Plotly will animate between (= zooming feel). For FDTD this looks bad; for an evolving phase-space plot it might be desired. Plan: leave it as-is (per-frame limits), but document; users can pin limits explicitly with `xlim()` once before the loop.

## Rollback plan

If Phase 2 ships and a curricular use exposes a fundamental issue (e.g. Plotly's animation runtime can't handle our trace shape), we can:
- Keep the `frame()` builtin (it's just a snapshot, harmless).
- Replace the `saveanim` body with the per-frame SVG fallback (the current workaround) until we redesign.
- No data migration required — `saveanim` writes one file per call, no persistent state.

This is unlikely; Plotly's animation API has been stable for years and our trace shapes (line series, heatmap, contour, quiver) are all in their canonical examples.

## Effort estimate

| Phase | Code | Tests | Docs | Total |
|---|---|---|---|---|
| 1 — frame buffer + `frame()` | ~150 LOC | ~80 LOC | ~40 LOC | ½ day |
| 2 — `saveanim` + Plotly emitter | ~300 LOC | ~120 LOC | ~50 LOC | 1 day |
| 3 — example + notebook + gallery | ~80 LOC | n/a | ~150 LOC prose | ½ day |
| 4 — curriculum integration | ~100 LOC across two lessons | n/a | request status flip | ½ day (in `rustlab_em`) |
| **Total upstream (Phases 1–3)** | **~530 LOC** | **~200 LOC** | **~240 LOC** | **2 days** |

## Follow-ups (post-landing, not part of this plan)

- **Option B — GIF / MP4 export.** Add `gif` crate (pure-Rust MIT/Apache, ~1500 LOC). Dispatch in `saveanim` based on path extension: `.gif` → rasterize each frame via `BitMapBackend`, encode with `gif`. Cost: seconds per frame at 1080p. Worth it only when a concrete external-share use case lands.
- **Delta-frame emission.** When the only field that changes between frames is `z` (heatmap) or `y` (line series), emit just the changing field in the Plotly `frames[i].data[j]`. Drops HTML size by ~3–5× for FDTD-scale animations.
- **Streaming flush.** For animations bigger than browser memory, stream frames to disk as they're captured rather than buffering. Requires a different file format (one HTML per N frames + a manifest) or a binary side-car. Probably not needed for curriculum.
- **`surf` animation hardening.** The Plotly emitter already handles 3-D scenes; verify they round-trip per-frame and document the result.

---

**Next action:** review this plan; on approval, start Phase 1 in a single-commit, tests-included PR.
