//! Multi-frame animation support.
//!
//! Two thread-local APIs:
//! - `push_frame()` — clones the current `FIGURE` into the per-thread frame
//!   buffer, then strips trace data from `FIGURE` so the next iteration starts
//!   with a clean canvas while keeping subplot layout, axes, titles, and
//!   limits intact.
//! - `render_animation_html(path, fps)` — drains the buffer and writes a
//!   single self-contained Plotly HTML document with play/pause buttons and
//!   a per-frame slider.
//!
//! The buffer is cleared whenever `figure()` / `figure(N)` is called, so the
//! "start a new animation" pattern is `figure(); for ... frame() ... end;
//! saveanim(...)`.

use std::cell::RefCell;

use crate::error::PlotError;
use crate::figure::{FigureState, FIGURE};
use crate::html::render_figure_plotly_div;
use crate::theme::{Theme, ThemeColors};

/// Output format requested by a notebook `saveanim()` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotebookAnimationFormat {
    /// Plotly HTML — embedded inline in HTML notebooks via
    /// `render_animation_inline`; placeholder note in Markdown / LaTeX.
    Html,
    /// Animated GIF — written to disk under the notebook plot directory
    /// and embedded via `<img src=...>` (HTML) or `![..](..)` (Markdown).
    Gif,
}

/// One captured animation inside a notebook code block. Mirrors the role
/// `FigureState` plays for static plots.
#[derive(Debug, Clone)]
pub struct NotebookAnimation {
    pub frames: Vec<FigureState>,
    pub fps: f64,
    pub format: NotebookAnimationFormat,
}

thread_local! {
    /// Per-thread buffer of figure snapshots captured by `frame()`.
    static FRAMES: RefCell<Vec<FigureState>> = const { RefCell::new(Vec::new()) };

    /// Animations captured by `saveanim()` while running under
    /// `PlotContext::Notebook`. Drained by the notebook executor at
    /// end-of-block, mirroring `NOTEBOOK_FIGURES`.
    static NOTEBOOK_ANIMATIONS: RefCell<Vec<NotebookAnimation>> = const { RefCell::new(Vec::new()) };
}

/// Snapshot the current `FIGURE` into the frame buffer.
pub fn push_frame() {
    let snap = FIGURE.with(|f| f.borrow().clone());
    FRAMES.with(|v| v.borrow_mut().push(snap));
}

/// Empty the frame buffer.
pub fn clear_frames() {
    FRAMES.with(|v| v.borrow_mut().clear());
}

/// Number of frames currently buffered.
pub fn frames_len() -> usize {
    FRAMES.with(|v| v.borrow().len())
}

/// Drain the frame buffer and return the captured states.
pub fn take_frames() -> Vec<FigureState> {
    FRAMES.with(|v| std::mem::take(&mut *v.borrow_mut()))
}

/// Strip trace data from every subplot of the current `FIGURE` while
/// preserving subplot layout, axis labels, titles, limits, hold state, and
/// grid setting. Used by `frame()` so the loop body's next iteration starts
/// with an empty canvas without dropping the user's axis configuration.
pub fn clear_figure_traces() {
    FIGURE.with(|fig| {
        let mut fig = fig.borrow_mut();
        for sp in fig.subplots.iter_mut() {
            sp.series.clear();
            sp.heatmap = None;
            sp.surface = None;
            sp.contours.clear();
            sp.quivers.clear();
            sp.streamlines.clear();
        }
    });
}

/// Drain the frame buffer and write a Plotly HTML animation to `path`.
///
/// `fps` controls per-frame display duration (`1000/fps` ms). Errors out if
/// the buffer is empty.
pub fn render_animation_html(path: &str, fps: f64) -> Result<(), PlotError> {
    let frames = take_frames();
    if frames.is_empty() {
        return Err(PlotError::FileOutput(
            "render_animation_html: frame buffer is empty".to_string(),
        ));
    }
    let theme = Theme::default();
    let html = render_animation_doc(&frames, fps, theme.colors());
    std::fs::write(path, html).map_err(|e| PlotError::FileOutput(e.to_string()))
}

/// Build a self-contained Plotly HTML document for the given frames. The
/// document includes a CDN script tag, a body wrapper, and the animation
/// `<div>` from `render_animation_inline`. Used both by `render_animation_html`
/// (writes to disk) and by the notebook markdown emitter (writes per-anim
/// HTML files into the plot directory).
pub fn render_animation_doc(frames: &[FigureState], fps: f64, theme: &ThemeColors) -> String {
    let inner = render_animation_inline(frames, "plot", fps, theme);
    let mut html = String::with_capacity(4096 + inner.len());
    html.push_str(&format!(
        r##"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>RustLab Animation</title>
<script src="https://cdn.plot.ly/plotly-2.35.0.min.js"></script>
<style>
  body {{ margin: 0; background: {bg}; color: {text}; font-family: sans-serif; }}
  #plot {{ width: 100vw; height: 100vh; }}
</style>
</head>
<body>
"##,
        bg = theme.bg,
        text = theme.text,
    ));
    html.push_str(&inner);
    html.push_str(
        r##"</body>
</html>
"##,
    );
    html
}

/// Build a Plotly animation as an inline `<div>` + `<script>` fragment using
/// `div_id` as the target element. Used by the notebook HTML renderer to
/// embed animations alongside static plots, and by `render_animation_doc` for
/// the standalone-document path. No `<html>` / `<head>` wrapper.
pub fn render_animation_inline(
    frames: &[FigureState],
    div_id: &str,
    fps: f64,
    theme: &ThemeColors,
) -> String {
    if frames.is_empty() {
        return String::new();
    }
    // Base figure = first frame. Re-uses the static plotly emitter so themes,
    // subplot domains, and trace ordering match the static `savefig` output.
    let base_div = render_figure_plotly_div(&frames[0], div_id, theme);

    // Per-frame trace JSON is extracted by emitting each frame to a private
    // div_id and slicing the data array literal out of the script. For
    // simplicity in v1 we emit each frame's full data array; delta-only
    // frames are a documented follow-up.
    let mut frame_blocks = String::new();
    for (idx, fr) in frames.iter().enumerate().skip(1) {
        let private_id = format!("__{div_id}_frame_{idx}");
        let frame_div = render_figure_plotly_div(fr, &private_id, theme);
        if let Some(data_arr) = extract_data_array(&frame_div, &private_id) {
            frame_blocks.push_str(&format!(
                "  {{ name: \"{idx}\", data: {data_arr} }},\n"
            ));
        }
    }

    let frame_duration_ms = if fps > 0.0 { 1000.0 / fps } else { 100.0 };
    let n_frames = frames.len();
    let js_var = div_id.replace('-', "_");

    let mut out = String::with_capacity(4096 + base_div.len() + frame_blocks.len());
    out.push_str(&base_div);
    out.push_str(&format!(
        r##"<script>
(function() {{
  var frames_{js_var} = [
{frame_blocks}  ];
  var slider_steps_{js_var} = [];
  for (var i = 0; i < {n_frames}; i++) {{
    slider_steps_{js_var}.push({{
      method: "animate",
      label: String(i),
      args: [[String(i)], {{
        mode: "immediate",
        frame: {{ duration: {dur}, redraw: true }},
        transition: {{ duration: 0 }}
      }}]
    }});
  }}
  var ready_{js_var} = function() {{
    if (frames_{js_var}.length > 0) {{
      Plotly.addFrames("{div_id}", frames_{js_var});
    }}
    Plotly.relayout("{div_id}", {{
      updatemenus: [{{
        type: "buttons",
        showactive: false,
        x: 0.0, y: -0.12, xanchor: "left", yanchor: "top",
        bgcolor: "{btn_bg}", bordercolor: "{btn_border}", font: {{ color: "{text}" }},
        buttons: [
          {{
            label: "Play",
            method: "animate",
            args: [null, {{
              mode: "immediate",
              fromcurrent: true,
              frame: {{ duration: {dur}, redraw: true }},
              transition: {{ duration: 0 }}
            }}]
          }},
          {{
            label: "Pause",
            method: "animate",
            args: [[null], {{
              mode: "immediate",
              frame: {{ duration: 0, redraw: false }},
              transition: {{ duration: 0 }}
            }}]
          }}
        ]
      }}],
      sliders: [{{
        active: 0,
        x: 0.12, y: -0.12, xanchor: "left", yanchor: "top", len: 0.85,
        currentvalue: {{
          prefix: "frame: ", visible: true, xanchor: "right",
          font: {{ color: "{text}" }}
        }},
        bgcolor: "{btn_bg}", bordercolor: "{btn_border}",
        font: {{ color: "{text}" }},
        pad: {{ t: 30, b: 10 }},
        steps: slider_steps_{js_var}
      }}]
    }});
  }};
  if (document.getElementById("{div_id}") && document.getElementById("{div_id}").data) {{
    ready_{js_var}();
  }} else {{
    var t_{js_var} = setInterval(function() {{
      var el = document.getElementById("{div_id}");
      if (el && el.data) {{ clearInterval(t_{js_var}); ready_{js_var}(); }}
    }}, 50);
  }}
}})();
</script>
"##,
        div_id = div_id,
        js_var = js_var,
        frame_blocks = frame_blocks,
        n_frames = n_frames,
        dur = frame_duration_ms as u64,
        btn_bg = theme.plot_bg,
        btn_border = theme.border,
        text = theme.text,
    ));

    out
}

// ─── GIF renderer ────────────────────────────────────────────────────────

/// Drain the frame buffer and write an animated GIF to `path`.
///
/// Each frame is rasterized via `render_figure_state_to_rgb_buffer` and
/// quantized through the `gif` crate's NeuQuant encoder (per-frame palette,
/// 256 colours). `fps` controls per-frame display duration; GIF stores
/// delays in centiseconds, so values above ~100 fps round to the same 1 cs
/// floor.
///
/// Output sizes are roughly half the equivalent Plotly HTML for typical
/// curriculum-scale heatmap demos (e.g. 60 frames at 100×100 → ~5 MB GIF
/// vs ~13 MB Plotly HTML), and the GIF embeds inline in GitHub markdown,
/// PDFs (via the `animate` LaTeX package), and plain `<img>` tags.
pub fn render_animation_gif(path: &str, fps: f64) -> Result<(), PlotError> {
    let frames = take_frames();
    if frames.is_empty() {
        return Err(PlotError::FileOutput(
            "render_animation_gif: frame buffer is empty".to_string(),
        ));
    }
    write_animation_gif(path, &frames, fps)
}

/// Write the given frames out as an animated GIF without touching the
/// thread-local buffer. Used by the notebook renderer to flush captured
/// animations.
pub fn write_animation_gif(
    path: &str,
    frames: &[FigureState],
    fps: f64,
) -> Result<(), PlotError> {
    if frames.is_empty() {
        return Err(PlotError::FileOutput(
            "write_animation_gif: no frames".to_string(),
        ));
    }
    let theme = Theme::default();
    let theme_colors = theme.colors();

    // Render the first frame to capture canvas dimensions; subsequent frames
    // share the same width/height (subplot grid is canonical across an
    // animation per the plan).
    let (first_buf, w, h) = crate::file::render_figure_state_to_rgb_buffer(&frames[0], theme_colors)?;
    let w_u16: u16 = w
        .try_into()
        .map_err(|_| PlotError::FileOutput(format!("render_animation_gif: width {w} exceeds GIF u16 limit (65535)")))?;
    let h_u16: u16 = h
        .try_into()
        .map_err(|_| PlotError::FileOutput(format!("render_animation_gif: height {h} exceeds GIF u16 limit (65535)")))?;

    let file = std::fs::File::create(path).map_err(|e| PlotError::FileOutput(e.to_string()))?;
    let mut encoder = gif::Encoder::new(file, w_u16, h_u16, &[])
        .map_err(|e| PlotError::FileOutput(format!("gif encoder init: {e}")))?;
    encoder
        .set_repeat(gif::Repeat::Infinite)
        .map_err(|e| PlotError::FileOutput(format!("gif set_repeat: {e}")))?;

    // Per-frame delay in centiseconds (GIF unit). Floor at 1 cs (~100 fps).
    let delay_cs: u16 = if fps > 0.0 {
        ((100.0 / fps).round() as i32).clamp(1, u16::MAX as i32) as u16
    } else {
        10
    };

    // Convert RGB bytes (3 bpp) → indexed (1 bpp + palette) via NeuQuant.
    // `from_rgb_speed`: speed 1 = highest quality, 30 = fastest. 10 is a
    // good trade-off and is what `gif` itself recommends in its docs.
    let speed = 10;
    let mut frame = gif::Frame::from_rgb_speed(w_u16, h_u16, &first_buf, speed);
    frame.delay = delay_cs;
    frame.dispose = gif::DisposalMethod::Background;
    encoder
        .write_frame(&frame)
        .map_err(|e| PlotError::FileOutput(format!("gif write_frame: {e}")))?;

    for fr in frames.iter().skip(1) {
        let (buf, fw, fh) =
            crate::file::render_figure_state_to_rgb_buffer(fr, theme_colors)?;
        if fw != w || fh != h {
            return Err(PlotError::FileOutput(format!(
                "render_animation_gif: frame size {fw}x{fh} differs from base {w}x{h}",
            )));
        }
        let mut frame = gif::Frame::from_rgb_speed(w_u16, h_u16, &buf, speed);
        frame.delay = delay_cs;
        frame.dispose = gif::DisposalMethod::Background;
        encoder
            .write_frame(&frame)
            .map_err(|e| PlotError::FileOutput(format!("gif write_frame: {e}")))?;
    }

    Ok(())
}

// ─── Notebook capture (parallel to NOTEBOOK_FIGURES in figure.rs) ────────

/// Drain the frame buffer into the notebook animation capture queue.
/// Used by `saveanim()` when running under `PlotContext::Notebook` instead
/// of writing the user-supplied path. The notebook executor reads the queue
/// at end-of-block via `take_notebook_animations`.
pub fn push_notebook_animation_snapshot(fps: f64, format: NotebookAnimationFormat) {
    let frames = take_frames();
    if frames.is_empty() {
        return;
    }
    NOTEBOOK_ANIMATIONS.with(|v| {
        v.borrow_mut().push(NotebookAnimation {
            frames,
            fps,
            format,
        })
    });
}

/// Drain and return all notebook animations captured since the last call.
pub fn take_notebook_animations() -> Vec<NotebookAnimation> {
    NOTEBOOK_ANIMATIONS.with(|v| std::mem::take(&mut *v.borrow_mut()))
}

/// Discard any pending notebook animations without returning them.
pub fn clear_notebook_animations() {
    NOTEBOOK_ANIMATIONS.with(|v| v.borrow_mut().clear());
}

/// Pull the JS array literal that follows `var data_<js_var> = ` out of a
/// `render_figure_plotly_div` block, where `js_var` is `div_id` with hyphens
/// replaced by underscores (matching what `render_figure_plotly_div` emits).
/// Returns `None` on shape mismatch.
fn extract_data_array(div: &str, div_id: &str) -> Option<String> {
    let js_var = div_id.replace('-', "_");
    let needle = format!("var data_{js_var} = ");
    let start = div.find(&needle)? + needle.len();
    // Walk forward, balancing brackets, until we close the array.
    let bytes = div.as_bytes();
    if bytes.get(start) != Some(&b'[') {
        return None;
    }
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(div[start..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::figure::{HeatmapData, FIGURE};

    fn reset() {
        clear_frames();
        FIGURE.with(|f| f.borrow_mut().reset());
    }

    fn set_heatmap_2x2(value: f64) {
        FIGURE.with(|f| {
            let mut fig = f.borrow_mut();
            let sp = fig.current_mut();
            sp.heatmap = Some(HeatmapData {
                z: vec![vec![value, value], vec![value, value]],
                colorscale: "viridis".to_string(),
                ..Default::default()
            });
        });
    }

    #[test]
    fn push_frame_increments_buffer() {
        reset();
        set_heatmap_2x2(1.0);
        push_frame();
        assert_eq!(frames_len(), 1);
        push_frame();
        assert_eq!(frames_len(), 2);
    }

    #[test]
    fn clear_figure_traces_keeps_layout() {
        reset();
        FIGURE.with(|f| {
            let mut fig = f.borrow_mut();
            fig.hold = true;
            let sp = fig.current_mut();
            sp.title = "Wave".to_string();
            sp.xlabel = "x".to_string();
            sp.xlim = (Some(-1.0), Some(1.0));
        });
        set_heatmap_2x2(2.0);
        clear_figure_traces();
        FIGURE.with(|f| {
            let fig = f.borrow();
            let sp = fig.current();
            assert_eq!(sp.title, "Wave");
            assert_eq!(sp.xlabel, "x");
            assert_eq!(sp.xlim, (Some(-1.0), Some(1.0)));
            assert!(sp.heatmap.is_none());
            assert!(fig.hold, "hold should survive clear");
        });
    }

    #[test]
    fn take_frames_drains_buffer() {
        reset();
        set_heatmap_2x2(1.0);
        push_frame();
        push_frame();
        let drained = take_frames();
        assert_eq!(drained.len(), 2);
        assert_eq!(frames_len(), 0);
    }

    #[test]
    fn render_animation_html_errors_on_empty_buffer() {
        reset();
        let tmp = std::env::temp_dir().join("rustlab_anim_empty.html");
        let res = render_animation_html(tmp.to_str().unwrap(), 30.0);
        assert!(res.is_err(), "empty buffer should error");
    }

    #[test]
    fn render_animation_html_writes_file_with_frames_block() {
        reset();
        for v in [1.0, 2.0, 3.0] {
            set_heatmap_2x2(v);
            push_frame();
            clear_figure_traces();
        }
        let tmp = std::env::temp_dir().join("rustlab_anim_three.html");
        render_animation_html(tmp.to_str().unwrap(), 10.0).expect("render");
        let body = std::fs::read_to_string(&tmp).expect("read back");
        assert!(body.contains("Plotly.newPlot("), "must contain Plotly init");
        assert!(body.contains("var frames_plot = ["), "must declare frames array");
        assert!(body.contains(r#"name: "1""#), "frame name 1");
        assert!(body.contains(r#"name: "2""#), "frame name 2");
        // Frame 0 is the base; only frames 1..N appear in the addFrames block.
        assert!(!body.contains(r#"name: "0""#), "frame 0 stays as base");
        assert!(body.contains("Play"), "play button");
        assert!(body.contains("Pause"), "pause button");
        assert!(body.contains("sliders:"), "slider config");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn render_animation_html_clears_buffer_on_success() {
        reset();
        set_heatmap_2x2(1.0);
        push_frame();
        push_frame();
        let tmp = std::env::temp_dir().join("rustlab_anim_clear.html");
        render_animation_html(tmp.to_str().unwrap(), 10.0).expect("render");
        assert_eq!(frames_len(), 0, "render should drain buffer");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn render_animation_gif_writes_gif89a_header() {
        reset();
        for v in [0.2, 0.5, 0.8] {
            set_heatmap_2x2(v);
            push_frame();
            clear_figure_traces();
        }
        let tmp = std::env::temp_dir().join("rustlab_anim_three.gif");
        render_animation_gif(tmp.to_str().unwrap(), 10.0).expect("render");
        let bytes = std::fs::read(&tmp).expect("read back");
        assert_eq!(&bytes[..6], b"GIF89a", "gif header missing");
        // Three frames at 900×500 each, NeuQuant'd. Typical sizes 15–30 KB
        // for the simple synthetic heatmap; just sanity-check it's not
        // empty / header-only.
        assert!(
            bytes.len() > 5_000,
            "gif body unexpectedly small: {} bytes",
            bytes.len()
        );
        assert_eq!(frames_len(), 0, "render should drain buffer");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn render_animation_gif_errors_on_empty_buffer() {
        reset();
        let tmp = std::env::temp_dir().join("rustlab_anim_gif_empty.gif");
        let res = render_animation_gif(tmp.to_str().unwrap(), 30.0);
        assert!(res.is_err(), "empty buffer should error");
    }
}
