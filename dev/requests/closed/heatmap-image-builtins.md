# Plan: Add `heatmap()` and `image()` Builtins

**Status**: Landed — `heatmap()` and `image()` are registered builtins in `crates/rustlab-script/src/eval/builtins.rs`; demo in `examples/notebooks/heatmap_image.md`.  
**Date**: 2026-04-18 (revised 2026-05-01)

## Revision notes (2026-05-01)

- `saveimagesc` / `builtin_saveimagesc` was removed before this plan was authored; all references below are dropped.
- `HeatmapData` is now constructed in nine call sites (one production, eight tests) — Phase 1 site list updated. The animation-export PR (`60592fe`) added one new test site in `animation.rs`.
- Line numbers refreshed against `main` (HEAD = `60592fe`).
- Design decision locked: the new label fields live on `HeatmapData` itself ("option A") rather than reusing `SubplotState.x_labels`. Rationale: heatmap labels die with the heatmap on `hold off`, so coupling them to `HeatmapData`'s lifetime avoids the manual-clear footgun the bar chart already deals with (`file.rs:1540`). The bar chart keeps using `SubplotState.x_labels` unchanged.
- Added a real-matrix sanity check for `image(R,G,B)` (Phase 3) — silently dropping non-zero imag parts is unfriendly.

### Decisions captured 2026-05-01

1. **Y-axis orientation: row 0 at top** for `heatmap()` and `image()` (the standard image/data orientation). Today's `imagesc` SVG already flips this way (`file.rs:920`); Plotly HTML for `imagesc` does **not** (no `y` array, no `autorange: reversed`), so HTML currently shows row 0 at the bottom — a pre-existing SVG/HTML divergence. This plan emits `yaxis.autorange: "reversed"` for `Heatmap` and `ImageRgba` kinds so HTML matches SVG. **`Imagesc` HTML output is left unchanged in this PR**; the existing divergence is tracked as a follow-up rather than fixed implicitly here.
2. **SVG axis tick labels for `Heatmap` kind: implement.** Follow the existing bar-chart pattern (`file.rs:311-335`) — pass label vectors into `x_label_formatter` / `y_label_formatter` closures and request `ncols`/`nrows` ticks. Edge-aligned ticks (cells run `[c, c+1]`, ticks land at integer cell boundaries) — same trade-off the bar chart already accepts. HTML/Plotly stays cell-centered natively via the `x:`/`y:` text arrays.
3. **Empty-data handling: return an error.** `heatmap([])` and `image([])` (and any zero-row or zero-col matrix) return `ScriptError::type_err("heatmap: matrix must be non-empty")` / equivalent for `image`. Today's `imagesc_terminal` returns `PlotError::EmptyData`; the new builtins should fail at the script-error layer before pushing any FIGURE state.

## Context

rustlab has `imagesc(M)` which auto-scales matrix values to a colormap. Two related visualization functions are missing:

- **`heatmap()`** — like `imagesc` but with categorical axis labels (row/column names), more like a data table visualization
- **`image()`** — raw pixel display with no min/max normalization; values 0-255 map directly to colors; supports RGB via three matrices

Both must work across all output backends: **notebooks** (Plotly HTML), **savefig** (SVG/PNG/HTML), and **rustlab-viewer** (egui). Terminal rendering is **best-effort** (reuse existing `imagesc_terminal` approach where possible; labels in the TUI are not required).

## Design Decisions

1. **Extend `HeatmapData` rather than adding new structs.** All four backends already dispatch on `SubplotState.heatmap: Option<HeatmapData>`. A `HeatmapKind` enum discriminates behavior at render time.

2. **`image()` RGB: pre-merge to RGBA in the builtin.** The viewer already expects RGBA (`WireHeatmap`), Plotly `type: "image"` wants `[r,g,b,a]` per pixel, and plotters draws colored rectangles. Storing three separate z-matrices provides no benefit. An `rgba: Option<Vec<u8>>` field on `HeatmapData` carries the pre-rendered pixels.

3. **Plotly `type: "image"` for image().** Native Plotly trace type with `z = [[[r,g,b,a], ...], ...]`. Simpler than base64 PNG and gives zoom/pan/hover for free. Supported since Plotly.js 1.54 (rustlab uses 2.35.0).

4. **Refactor `imagesc_terminal` to split FIGURE push from TUI draw.** Both new builtins need to push richer `HeatmapData` than `imagesc_terminal` would, then separately trigger the TUI render.

## Existing Architecture (read this first)

The heatmap data flow is:

```
User Code (imagesc/heatmap/image)
    |
builtin function (builtins.rs)
    |
Pushes HeatmapData into FIGURE thread-local state (figure.rs)
    |
    +---> Terminal: colormap_rgb(t) -> colored blocks via ratatui (ascii.rs)
    +---> HTML/Plotly: z matrix -> JSON, colorscale mapping (html.rs)
    +---> SVG/PNG: render_imagesc_to_backend -> plotters rectangles (file.rs)
    +---> Report: auto-capture figure -> HTML export (report.rs)
    +---> Viewer: normalize z -> colormap_rgb -> RGBA pixels
    |       -> ViewerMsg::PanelHeatmap { WireHeatmap {width, height, rgba} }
    |       -> rustlab-viewer creates egui texture (viewer_live.rs -> app.rs)
    +---> Notebook: executor captures FIGURE state after code block,
            renders via render_figure_plotly_div (execute.rs -> html.rs)
```

### Key structs and locations

| What | File | Line |
|------|------|------|
| `HeatmapData { z, colorscale }` | `crates/rustlab-plot/src/figure.rs` | ~100 |
| `SubplotState.heatmap: Option<HeatmapData>` | `crates/rustlab-plot/src/figure.rs` | ~194 |
| `SubplotState.x_labels: Option<Vec<String>>` | `crates/rustlab-plot/src/figure.rs` | ~192 |
| `colormap_rgb(t, name) -> (u8,u8,u8)` | `crates/rustlab-plot/src/figure.rs` | ~389 |
| `imagesc_terminal()` | `crates/rustlab-plot/src/ascii.rs` | ~367 |
| `builtin_imagesc()` | `crates/rustlab-script/src/eval/builtins.rs` | ~2855 |
| Plotly heatmap trace generation | `crates/rustlab-plot/src/html.rs` | ~164 |
| SVG/PNG heatmap rendering | `crates/rustlab-plot/src/file.rs` | ~72, ~280 |
| Viewer RGBA pre-rendering | `crates/rustlab-plot/src/viewer_live.rs` | ~261 |
| `WireHeatmap { width, height, rgba }` | `crates/rustlab-proto/src/lib.rs` | ~103 |
| Viewer egui texture creation | `crates/rustlab-viewer/src/figure.rs` | ~133 |
| Notebook figure capture | `crates/rustlab-notebook/src/execute.rs` | ~59 |

### How `imagesc_terminal` works today

1. Extracts `.norm()` of each complex element -> `vals: Vec<f64>`
2. Computes min/max for normalization
3. Builds `z: Vec<Vec<f64>>` row-major
4. **Pushes** `HeatmapData { z, colorscale }` into `FIGURE` thread-local (`ascii.rs:383-398`)
5. Early-returns if Notebook/Headless context or non-terminal stdout
6. Renders colored blocks via crossterm/ratatui using `colormap_rgb(t, colormap)`

The problem for new builtins: step 4 pushes a plain `HeatmapData`. The new builtins need to push enriched data (with labels or RGBA), so they can't delegate the FIGURE push to `imagesc_terminal`.

## Phase 1: Extend data model

**File: `crates/rustlab-plot/src/figure.rs`**

Add enum before `HeatmapData` (~line 83):

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum HeatmapKind {
    /// imagesc: continuous values, min/max normalization, colormap applied
    Imagesc,
    /// heatmap: like Imagesc but with categorical axis labels
    Heatmap,
    /// image: raw RGBA pixel data, no normalization
    ImageRgba,
}
```

Extend `HeatmapData`:

```rust
pub struct HeatmapData {
    pub z: Vec<Vec<f64>>,
    pub colorscale: String,
    pub kind: HeatmapKind,                  // NEW
    pub x_labels: Option<Vec<String>>,      // NEW - column labels (Heatmap kind)
    pub y_labels: Option<Vec<String>>,      // NEW - row labels (Heatmap kind)
    pub rgba: Option<Vec<u8>>,              // NEW - pre-rendered pixels (ImageRgba kind)
    pub rgba_width: u32,                    // NEW
    pub rgba_height: u32,                   // NEW
}
```

**Add a `Default` impl** so test sites can migrate with `..Default::default()`:

```rust
impl Default for HeatmapData {
    fn default() -> Self {
        Self {
            z: Vec::new(),
            colorscale: String::new(),
            kind: HeatmapKind::Imagesc,
            x_labels: None,
            y_labels: None,
            rgba: None,
            rgba_width: 0,
            rgba_height: 0,
        }
    }
}
```

**Update existing `HeatmapData` construction sites.** There are nine — one production, eight in test modules:

| Site | Kind | Migration |
|---|---|---|
| `crates/rustlab-plot/src/ascii.rs:394` (`imagesc_terminal`) | production | Set `kind: HeatmapKind::Imagesc` and other new fields explicitly |
| `crates/rustlab-plot/src/html.rs:752` | test | `..Default::default()` |
| `crates/rustlab-plot/src/animation.rs:448` | test (`set_heatmap_2x2`) | `..Default::default()` |
| `crates/rustlab-plot/src/file.rs:1552, 1617, 1642, 1723, 1898, 2186, 2224` | test | `..Default::default()` |

**Export `HeatmapKind`** from `crates/rustlab-plot/src/lib.rs` (alongside existing `HeatmapData` export).

## Phase 2: Refactor terminal rendering

**File: `crates/rustlab-plot/src/ascii.rs`**

Split `imagesc_terminal` (~line 290) into two functions:

### `render_heatmap_tui()`
Extract the pure TUI rendering portion (lines ~324-380) into a standalone function:
```rust
pub fn render_heatmap_tui(
    vals: &[f64], nrows: usize, ncols: usize,
    min_v: f64, range: f64, title: &str, colormap: &str,
) -> Result<(), PlotError>
```
This does the crossterm EnterAlternateScreen, ratatui colored blocks, wait-for-key, restore. Does NOT touch FIGURE state. Includes the existing `IsTerminal` and `PlotContext::Notebook` early-return guards.

### Refactored `imagesc_terminal()`
Becomes:
1. Extract vals, compute min/max/range (existing code)
2. Build z-matrix, push `HeatmapData { kind: Imagesc, ... }` into FIGURE (existing code)
3. Call `render_heatmap_tui(vals, nrows, ncols, min_v, range, title, colormap)`

### `render_image_tui()`
New function for `image()` best-effort terminal rendering:
```rust
pub fn render_image_tui(
    rgba: &[u8], width: usize, height: usize, title: &str,
) -> Result<(), PlotError>
```
Same approach as `render_heatmap_tui` but reads `(r, g, b)` directly from the RGBA buffer instead of computing via colormap. Include `IsTerminal` + `Notebook` guards.

Export all three from `crates/rustlab-plot/src/lib.rs`.

## Phase 3: Add builtins

**File: `crates/rustlab-script/src/eval/builtins.rs`**

### `heatmap()`

Register (~line 163, after `imagesc`): `r.register("heatmap", builtin_heatmap);`

Signatures:
- `heatmap(M)` - numeric indices as labels
- `heatmap(M, "title")` - with title
- `heatmap(xlabels, ylabels, M)` - categorical string array labels
- `heatmap(xlabels, ylabels, M, "title")`
- `heatmap(xlabels, ylabels, M, "title", "colormap")`

Where `xlabels`/`ylabels` are string arrays like `["Mon", "Tue", "Wed"]`.

Implementation:
1. Parse args - detect leading `Value::StringArray` (pattern from `builtin_bar:7085`) vs matrix/scalar
2. Extract matrix, take `.norm()` for complex support (same as imagesc)
3. **Empty-matrix check**: if `nrows == 0 || ncols == 0`, return `ScriptError::type_err("heatmap: matrix must be non-empty")` *before* pushing FIGURE state
4. Validate label lengths: `xlabels.len() == ncols`, `ylabels.len() == nrows` — error otherwise
5. Build `z: Vec<Vec<f64>>` from matrix values
6. Push into FIGURE:
   ```rust
   sp.heatmap = Some(HeatmapData {
       z,
       colorscale: colormap.to_string(),
       kind: HeatmapKind::Heatmap,
       x_labels: Some(xlabels), // or None if no labels provided
       y_labels: Some(ylabels), // or None if no labels provided
       rgba: None,
       rgba_width: 0,
       rgba_height: 0,
   });
   ```
6. Call `render_heatmap_tui()` for terminal display (best-effort, no labels in TUI)
7. Call `sync_figure_outputs()`

**Note on string arrays**: Match the pattern in `builtin_bar` (~line 7085). Branch on `if let Value::StringArray(labels) = &args[0]` to detect the labeled form, then `labels.clone()` to get an owned `Vec<String>` for the `HeatmapData`.

### `image()`

Register: `r.register("image", builtin_image);`

Signatures:
- `image(M)` - grayscale, values clamped 0-255
- `image(M, "colormap")` - single channel mapped through colormap
- `image(R, G, B)` - three matrices for true-color RGB

Implementation:
1. Parse args:
   - 1 arg (matrix) -> grayscale
   - 2 args (matrix, string) -> matrix + colormap
   - 3 args (matrix, matrix, matrix) -> RGB; require all three to share `(nrows, ncols)`, else `ScriptError::type_err`
2. **Empty-matrix check**: if `nrows == 0 || ncols == 0` on any input, return `ScriptError::type_err("image: matrix must be non-empty")` *before* pushing FIGURE state
3. Build RGBA buffer:
   - **Grayscale**: `v = val.norm().clamp(0.0, 255.0) as u8; [v, v, v, 255]`
   - **Colormap**: `t = val.norm().clamp(0.0, 255.0) / 255.0; colormap_rgb(t, name) -> [r, g, b, 255]`
   - **RGB**: `r = R[i][j].re.clamp(0.0, 255.0) as u8` (use `.re` not `.norm()` for RGB channels), same for g, b; `[r, g, b, 255]`. **Reject** matrices with non-trivial imaginary parts: if any `|im| > 1e-9` in R/G/B, return `ScriptError::type_err("image: RGB channels must be real matrices")` rather than silently dropping the imag component.
4. Push into FIGURE:
   ```rust
   sp.heatmap = Some(HeatmapData {
       z: vec![],  // not used for ImageRgba
       colorscale: String::new(),
       kind: HeatmapKind::ImageRgba,
       x_labels: None,
       y_labels: None,
       rgba: Some(rgba_buf),
       rgba_width: ncols as u32,
       rgba_height: nrows as u32,
   });
   ```
5. Call `render_image_tui()` for terminal (best-effort)
6. Call `sync_figure_outputs()`

## Phase 4: Update rendering backends

### 4a. Plotly/HTML

**File: `crates/rustlab-plot/src/html.rs`**

In `render_figure_plotly_div`, expand the heatmap trace block (~line 164) to branch on `hm.kind`:

**`Imagesc`** (existing): No change. Emits `type: "heatmap"` with z-matrix JSON.

**`Heatmap`**: Emit `type: "heatmap"` with z-matrix PLUS `x` and `y` text arrays:
```javascript
{
  z: [[1,2,3],[4,5,6]],
  x: ["Mon","Tue","Wed"],
  y: ["Alice","Bob"],
  type: "heatmap",
  colorscale: "Viridis",
  showscale: true,
  xaxis: "x", yaxis: "y"
}
```
Plotly handles categorical axes natively from text x/y arrays. Also emit `autorange: "reversed"` on the panel's `yaxis` layout entry for `Heatmap` kind so row 0 lands at the top (standard image/data orientation, matching what `imagesc` SVG already does at `file.rs:920`).

**`ImageRgba`**: Emit `type: "image"` trace. Build z as 3D JSON array where each pixel is `[r,g,b,a]`:
```javascript
{
  z: [[[r,g,b,a],[r,g,b,a],...], ...],
  type: "image",
  xaxis: "x", yaxis: "y"
}
```
No colorscale needed. **Keep** the `scaleanchor` square-aspect logic (`html.rs:150`) — images need fixed aspect or pixels stretch when the panel resizes. Also emit `autorange: "reversed"` on the panel's `yaxis` so row 0 is at the top.

### 4b. SVG/PNG

**File: `crates/rustlab-plot/src/file.rs`**

In `render_to_backend` heatmap branch (~line 72), branch on `hm.kind`:

**`Imagesc`**: No change (existing `render_imagesc_to_backend` call).

**`Heatmap`**: Same colored rectangles and same row-0-at-top y-flip as `Imagesc` (`y0 = y_hi - (r + 1) * cell_h`, `file.rs:920`). Adds categorical tick label rendering via plotters formatters.

The current `Imagesc` mesh setup at `file.rs:874-883` is one `chart.configure_mesh().disable_mesh()...draw()` chain. For `Heatmap` kind, branch the mesh configuration to install x/y label formatters that look up `hm.x_labels` / `hm.y_labels` by index. Pattern mirrors the bar chart's `x_label_formatter` at `file.rs:322-333`:

```rust
if let (Some(xlbls), Some(ylbls)) = (hm.x_labels.as_ref(), hm.y_labels.as_ref()) {
    let xlbls_c = xlbls.clone();
    let ylbls_c = ylbls.clone();
    let nrows_c = nrows;
    chart.configure_mesh()
        .disable_mesh()
        .axis_style(axis_style)
        .label_style(label_style.clone())
        .axis_desc_style(desc_style.clone())
        .x_desc(sp.xlabel.as_str())
        .y_desc(sp.ylabel.as_str())
        .x_labels(xlbls_c.len())
        .y_labels(ylbls_c.len())
        .x_label_formatter(&|v| {
            let rounded = v.round();
            if (*v - rounded).abs() > 1e-6 { return String::new(); }
            let idx = rounded as isize;
            if idx >= 0 && (idx as usize) < xlbls_c.len() {
                xlbls_c[idx as usize].clone()
            } else { String::new() }
        })
        .y_label_formatter(&|v| {
            // Row 0 is at the top: chart y = nrows means row 0; chart y = 0 means row nrows-1.
            let rounded = v.round();
            if (*v - rounded).abs() > 1e-6 { return String::new(); }
            let chart_y = rounded as isize;
            let row_idx = (nrows_c as isize) - 1 - chart_y;
            if row_idx >= 0 && (row_idx as usize) < ylbls_c.len() {
                ylbls_c[row_idx as usize].clone()
            } else { String::new() }
        })
        .draw().map_err(err)?;
} else {
    /* existing Imagesc mesh chain */
}
```

Where one of `x_labels` / `y_labels` is `Some` and the other is `None`, only that axis gets a formatter — the other uses the default numeric mesh. (Build the mesh chain conditionally; plotters' fluent API doesn't admit a clean "skip if None" so this may need a small helper or two parallel branches.)

Tick alignment is on cell edges, not centers — the same trade-off the bar chart accepts at `file.rs:322`. HTML output remains cell-centered via Plotly's `x:`/`y:` text arrays.

**`ImageRgba`**: New helper `render_image_rgba_to_backend`. Draw colored rectangles from RGBA directly using the same y-flip as `imagesc` so row 0 ends up at the top of the chart:

```rust
for r in 0..height {
    for c in 0..width {
        let off = (r * width + c) * 4;
        let color = RGBColor(rgba[off], rgba[off+1], rgba[off+2]);
        let x0 = c as f64;
        let y0 = height as f64 - (r as f64 + 1.0); // row 0 at top
        chart.draw_series(std::iter::once(Rectangle::new(
            [(x0, y0), (x0 + 1.0, y0 + 1.0)],
            color.filled(),
        )))?;
    }
}
```

**Also branch the colorbar gutter split** (`file.rs:836-855`). The current code splits a colorbar strip whenever `sp.heatmap.is_some()`. For `ImageRgba` this is meaningless — there's no scale to show. Gate the split on `Imagesc | Heatmap`:

```rust
let needs_colorbar = matches!(
    sp.heatmap.as_ref().map(|h| &h.kind),
    Some(HeatmapKind::Imagesc) | Some(HeatmapKind::Heatmap)
);
let (chart_area, cbar_info) = if needs_colorbar {
    /* existing split logic */
} else if sp.heatmap.is_some() {
    /* ImageRgba: square chart area, no gutter */
    /* compute side from min(data_w_avail, data_h_avail), shrink root to a square */
    (square_chart_area, None)
} else {
    (root.clone(), None)
};
```

The colorbar render at `file.rs:960` already guards on `cbar_info.is_some()`, so passing `None` for `ImageRgba` cleanly skips it.

### 4c. Viewer

**File: `crates/rustlab-plot/src/viewer_live.rs`**

In `send_figure_state` heatmap branch (~line 261), branch on `hm.kind`:

**`Imagesc`**: No change (existing normalize + colormap_rgb -> RGBA path).

**`Heatmap`**: Same RGBA rendering as Imagesc. The viewer shows the image as a texture. Categorical labels are not rendered in the viewer egui texture in v1 (acceptable - the viewer is a live preview, detailed labels show in HTML/savefig output).

**`ImageRgba`**: Use `hm.rgba` directly, skip normalization:
```rust
if hm.kind == HeatmapKind::ImageRgba {
    if let Some(ref rgba) = hm.rgba {
        conn.client.send_nowait(&ViewerMsg::PanelHeatmap {
            fig_id,
            panel: idx as u16,
            heatmap: WireHeatmap {
                width: hm.rgba_width,
                height: hm.rgba_height,
                rgba: rgba.clone(),
            },
        })?;
    }
}
```

**No changes needed** to:
- `crates/rustlab-proto/src/lib.rs` - `WireHeatmap` already carries arbitrary RGBA
- `crates/rustlab-viewer/src/app.rs` - already renders any RGBA texture it receives
- `crates/rustlab-viewer/src/figure.rs` - same

## Phase 5: REPL help entries

**File: `crates/rustlab-cli/src/commands/repl.rs`**

Add `HelpEntry` for `heatmap` and `image` after the `imagesc` entry (~line 260):

```rust
HelpEntry { name: "heatmap", brief: "Labeled heatmap with categorical axis labels",
    detail: "heatmap(M)\nheatmap(M, \"title\")\nheatmap(xlabels, ylabels, M)\nheatmap(xlabels, ylabels, M, \"title\")\nheatmap(xlabels, ylabels, M, \"title\", \"colormap\")\n\n  xlabels, ylabels: string arrays [\"Mon\",\"Tue\",\"Wed\"]\n  colormap: \"viridis\" (default), \"jet\", \"hot\", \"gray\"" },
HelpEntry { name: "image", brief: "Raw pixel display (no normalization, values 0-255)",
    detail: "image(M)              -- grayscale (values 0-255)\nimage(M, \"colormap\")  -- single channel with colormap\nimage(R, G, B)        -- true-color RGB (values 0-255)\n\nValues clamped to [0, 255]. No min/max normalization (unlike imagesc)." },
```

Add `"heatmap"` and `"image"` to the "Plotting" category list (~line 960).

## Files to Modify (summary)

| File | Change |
|------|--------|
| `crates/rustlab-plot/src/figure.rs` | Add `HeatmapKind` enum, extend `HeatmapData` with 5 new fields |
| `crates/rustlab-plot/src/lib.rs` | Export `HeatmapKind`, new TUI functions |
| `crates/rustlab-plot/src/ascii.rs` | Refactor `imagesc_terminal` into push + render; add `render_heatmap_tui`, `render_image_tui` |
| `crates/rustlab-script/src/eval/builtins.rs` | Add `builtin_heatmap`, `builtin_image`; register both; update 1 existing `HeatmapData` construction site |
| `crates/rustlab-plot/src/html.rs` | Branch on `HeatmapKind` for Plotly trace generation |
| `crates/rustlab-plot/src/file.rs` | Branch on `HeatmapKind` for SVG/PNG rendering |
| `crates/rustlab-plot/src/viewer_live.rs` | Branch on `HeatmapKind` for viewer RGBA dispatch |
| `crates/rustlab-cli/src/commands/repl.rs` | Add help entries + category for `heatmap`, `image` |

## Build Order

Phases must be done in order because of compile dependencies:
1. **Phase 1** (data model) - everything else depends on the new fields
2. **Phase 2** (ascii.rs refactor) - builtins call the new TUI functions
3. **Phase 3** (builtins) - can partially overlap with Phase 4
4. **Phase 4** (rendering backends) - html.rs, file.rs, viewer_live.rs are independent of each other
5. **Phase 5** (help entries) - independent, do last

Build and test after each phase to catch errors early.

## Verification

1. `cargo test --workspace` - all tests pass, existing imagesc behavior unchanged
2. Run `examples/matrix_ops.rlab` - existing imagesc still works
3. Notebook test: create a notebook with `heatmap()` and `image()` calls, `cargo run -- notebook render`, verify Plotly charts appear correctly in the HTML
4. `savefig` test:
   - `heatmap(["A","B","C"], ["X","Y"], [1,2,3;4,5,6], "Test"); savefig("/tmp/hm.svg")` - produces SVG
   - `image(randn(8,8)*128+128); savefig("/tmp/img.png")` - produces PNG
   - `M = randn(8,8)*128+128; image(M, M, M); savefig("/tmp/rgb.html")` - produces HTML with image
5. Viewer test: `viewer on; heatmap(eye(4)); image(randn(8,8)*128+128)` - both render in viewer window
6. Terminal test: `heatmap(eye(4))` and `image(randn(8,8)*128+128)` render colored blocks in REPL (best-effort)
