//! Titles, axis labels, and tick labels must not occupy the same pixels.
//!
//! SVG (and therefore PDF, PNG, and GIF, which rasterise this backend) is
//! checked by parsing the `<text>` elements plotters writes. HTML and
//! `notebook watch` share the Plotly layout; when Chrome is on `PATH` the
//! same stress figures are laid out in a headless page and the DOM boxes
//! are compared. The structural half of that layout (axis anchors, title
//! placement) is also pinned by unit tests in `html.rs`, which run in CI
//! without a browser.

use rustlab_plot::{
    render_figure_plotly_div, render_figure_state_to_file_themed, FigureState, HeatmapData,
    HeatmapKind, HeatmapOrigin, LineStyle, PlotKind, Series, SeriesColor, Theme,
};
use std::path::Path;
use std::process::Command;

fn line(x0: f64, x1: f64, y0: f64, y1: f64) -> Series {
    Series {
        label: String::new(),
        x_data: vec![x0, x1],
        y_data: vec![y0, y1],
        color: SeriesColor::Cyan,
        style: LineStyle::Solid,
        kind: PlotKind::Line,
    }
}

fn stress_figures() -> Vec<(&'static str, FigureState)> {
    let mut out = Vec::new();

    let mut single = FigureState::new();
    {
        let sp = single.current_mut();
        sp.title = "Lowpass magnitude response — tap-count sweep across Hann, Hamming, and Blackman windows".into();
        sp.xlabel = "Frequency (Hz)".into();
        sp.ylabel = "Magnitude (dB)".into();
        sp.series.push(line(0.0, 10.0, -1.0, 1.0));
    }
    out.push(("long-title", single));

    let mut multi = FigureState::new();
    {
        let sp = multi.current_mut();
        sp.title =
            "First line of a long plot title\nSecond line that should stay clear of the axes"
                .into();
        sp.xlabel = "Time (s)".into();
        sp.ylabel = "Voltage (V)".into();
        sp.series.push(line(0.0, 10.0, -1.0, 1.0));
    }
    out.push(("multiline", multi));

    let mut grid = FigureState::new();
    for (idx, name) in [
        (1, "Return loss"),
        (2, "Forward gain"),
        (3, "Isolation"),
        (4, "Output match"),
    ] {
        grid.set_subplot(2, 2, idx);
        let sp = grid.current_mut();
        sp.title = format!("{name} S{idx}");
        sp.xlabel = "Frequency (GHz)".into();
        sp.ylabel = "Magnitude (dB)".into();
        sp.series.push(line(0.0, 10.0, -1.0, 1.0));
    }
    out.push(("grid-2x2", grid));

    let mut rows = FigureState::new();
    for (idx, title) in [
        (1, "Periodogram (no window)"),
        (2, "Hann-Windowed Periodogram"),
        (3, "Welch's PSD (Hamming, 8 segments)"),
    ] {
        rows.set_subplot(3, 1, idx);
        let sp = rows.current_mut();
        sp.title = title.into();
        sp.ylabel = "dB".into();
        if idx == 3 {
            sp.xlabel = "Frequency".into();
        }
        sp.series.push(line(0.0, 10.0, -1.0, 1.0));
    }
    out.push(("grid-3x1", rows));

    let mut big = FigureState::new();
    {
        let sp = big.current_mut();
        sp.title = "Sparse vs dense — time vs unknowns".into();
        sp.xlabel = "Unknowns".into();
        sp.ylabel = "Elapsed microseconds".into();
        sp.series.push(line(1.0e5, 8.0e6, -2.0e6, 2.0e6));
    }
    out.push(("large-ticks", big));

    let mut thin = FigureState::new();
    {
        let sp = thin.current_mut();
        sp.title = "Nearly vertical locus".into();
        sp.xlabel = "Real".into();
        sp.ylabel = "Imag".into();
        sp.axis_equal = true;
        sp.series.push(line(-0.02, 0.02, -2.0, 2.0));
    }
    out.push(("thin-equal", thin));

    let mut heat = FigureState::new();
    {
        let z: Vec<Vec<f64>> = (0..8)
            .map(|r| (0..8).map(|c| (r + c) as f64).collect())
            .collect();
        let sp = heat.current_mut();
        sp.title = "Poisson solution at the grid centre".into();
        sp.xlabel = "x [m]".into();
        sp.ylabel = "y [m]".into();
        sp.heatmap = Some(HeatmapData {
            z,
            colorscale: "viridis".into(),
            kind: HeatmapKind::Imagesc,
            x_labels: None,
            y_labels: None,
            rgba: None,
            rgba_width: 0,
            rgba_height: 0,
            value_min: None,
            value_max: None,
            origin: HeatmapOrigin::Lower,
        });
    }
    out.push(("heatmap", heat));

    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Title,
    XLabel,
    YLabel,
    Tick,
    Other,
}

struct Box {
    text: String,
    role: Role,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

fn overlaps(a: &Box, b: &Box) -> bool {
    let ix = a.x1.min(b.x1) - a.x0.max(b.x0);
    let iy = a.y1.min(b.y1) - a.y0.max(b.y0);
    ix > 2.0 && iy > 2.0
}

/// Plotters SVG text. `y` is the top of the em box when `dy` is `0.76em`
/// (titles and x ticks) and the vertical centre when `dy` is `0.5ex`
/// (y ticks). A `rotate` turns a y-axis title vertical about its anchor.
fn svg_boxes(svg: &str) -> Vec<Box> {
    let mut out = Vec::new();
    let mut rest = svg;
    while let Some(start) = rest.find("<text") {
        let after = &rest[start..];
        let Some(end_tag) = after.find('>') else {
            break;
        };
        let Some(close) = after.find("</text>") else {
            break;
        };
        let attrs = &after[..end_tag];
        let body = after[end_tag + 1..close].trim();
        let text = body
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        rest = &after[close + 7..];
        if text.is_empty() {
            continue;
        }
        let fs = attr_f64(attrs, "font-size").unwrap_or(12.0);
        let x = attr_f64(attrs, "x").unwrap_or(0.0);
        let y = attr_f64(attrs, "y").unwrap_or(0.0);
        let anchor = attr_str(attrs, "text-anchor").unwrap_or("start");
        let dy = attr_str(attrs, "dy").unwrap_or("");
        let transform = attr_str(attrs, "transform").unwrap_or("");
        // Slightly under the real glyph width so a near-miss is not a failure.
        // A label drawn on top of another still overlaps by most of its box.
        let w = text.chars().count() as f64 * fs * 0.48;
        let h = fs * 0.95;
        let (x0, x1) = match anchor {
            "middle" => (x - w / 2.0, x + w / 2.0),
            "end" => (x - w, x),
            _ => (x, x + w),
        };
        let (mut x0, mut y0, mut x1, mut y1) = if dy.contains("0.5ex") && !dy.starts_with('-') {
            (x0, y - h / 2.0, x1, y + h / 2.0)
        } else {
            (x0, y, x1, y + h)
        };
        if transform.contains("rotate") {
            let cx = x;
            let cy = y;
            let half_l = w / 2.0;
            let half_t = h / 2.0;
            x0 = cx - half_t;
            x1 = cx + half_t;
            y0 = cy - half_l;
            y1 = cy + half_l;
        }
        let role = if transform.contains("rotate") {
            Role::YLabel
        } else if dy.contains("-0.5ex") {
            Role::XLabel
        } else if fs >= 16.0 {
            Role::Title
        } else if dy.contains("0.76em") && fs < 14.0 {
            Role::Tick
        } else if dy.contains("0.5ex") {
            Role::Tick
        } else {
            Role::Other
        };
        out.push(Box {
            text,
            role,
            x0,
            y0,
            x1,
            y1,
        });
    }
    out
}

fn attr_str<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let i = attrs.find(&key)?;
    let rest = &attrs[i + key.len()..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

fn attr_f64(attrs: &str, name: &str) -> Option<f64> {
    attr_str(attrs, name)?.parse().ok()
}

fn assert_no_svg_overlap(name: &str, svg: &str) {
    let boxes = svg_boxes(svg);
    let mut hits = Vec::new();
    for i in 0..boxes.len() {
        for j in (i + 1)..boxes.len() {
            let a = &boxes[i];
            let b = &boxes[j];
            // Adjacent tick labels on a healthy axis can kiss at the 2px
            // threshold when the estimate is a little wide; title/label
            // collisions are the bug. Tick/tick still fails when they
            // share a point (the collapsed equal-aspect axis).
            let both_ticks = a.role == Role::Tick && b.role == Role::Tick;
            if both_ticks {
                let same_point = (a.x0 - b.x0).abs() < 1.0 && (a.y0 - b.y0).abs() < 1.0;
                if !same_point {
                    continue;
                }
            }
            if overlaps(a, b) {
                hits.push(format!(
                    "{:?} {:?} {:?} / {:?} {:?}",
                    a.role,
                    a.text,
                    (a.x0, a.y0),
                    b.role,
                    b.text
                ));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "{name} SVG text overlaps:\n{}",
        hits.join("\n")
    );
}

#[test]
fn svg_stress_text_does_not_overlap() {
    let dir = std::env::temp_dir().join(format!("rustlab-label-overlap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let theme = Theme::default().colors();
    for (name, fig) in stress_figures() {
        let path = dir.join(format!("{name}.svg"));
        render_figure_state_to_file_themed(&fig, path.to_str().unwrap(), theme).unwrap();
        let svg = std::fs::read_to_string(&path).unwrap();
        assert!(svg.contains("<text"), "{name} produced an SVG with no text");
        assert_no_svg_overlap(name, &svg);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

fn chrome_bin() -> Option<String> {
    for name in [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
    ] {
        if Command::new(name).arg("--version").output().is_ok() {
            return Some(name.to_string());
        }
    }
    None
}

/// Headless Chrome lays out the Plotly HTML the notebook watch server and
/// the static HTML export both use, then compares text bounding boxes.
#[test]
fn plotly_stress_text_does_not_overlap_in_chrome() {
    let Some(chrome) = chrome_bin() else {
        eprintln!("skip plotly overlap: chrome not on PATH");
        return;
    };
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("skip plotly overlap: node not on PATH");
        return;
    }
    let dir = std::env::temp_dir().join(format!("rustlab-plotly-overlap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let plotly_src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../rustlab-notebook/assets/vendor/plotly/plotly.min.js");
    std::fs::copy(&plotly_src, dir.join("plotly.min.js")).unwrap();

    let theme = Theme::default().colors();
    let mut body = String::new();
    for (name, fig) in stress_figures() {
        let div = render_figure_plotly_div(&fig, &format!("plot-{name}"), theme);
        let height = 450 + fig.subplot_rows.saturating_sub(1) * 350;
        body.push_str(&format!(
            "<h2>{name}</h2><div class=\"plot-container\" style=\"width:880px;height:{height}px\">{div}</div>\n"
        ));
    }
    let html = PAGE_HTML
        .replace("@@BG@@", theme.plot_bg)
        .replace("@@BODY@@", &body);
    let page = dir.join("index.html");
    std::fs::write(&page, html).unwrap();

    let script = dir.join("check.mjs");
    std::fs::write(
        &script,
        r#"
import { spawn } from "node:child_process";
import { setTimeout as delay } from "node:timers/promises";
const chromeBin = process.argv[2];
const url = process.argv[3];
const port = 9344 + (process.pid % 1000);
const chrome = spawn(chromeBin, ["--headless=new","--no-sandbox","--disable-gpu","--disable-dev-shm-usage",`--remote-debugging-port=${port}`,`--user-data-dir=/tmp/rl-overlap-${process.pid}`], {stdio:"ignore"});
let ws;
try {
  let info;
  for (let i = 0; i < 50; i++) {
    try { info = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json(); break; }
    catch { await delay(100); }
  }
  if (!info) { console.error("devtools down"); process.exit(2); }
  ws = new WebSocket(info.webSocketDebuggerUrl);
  await new Promise((res, rej) => { ws.addEventListener("open", res); ws.addEventListener("error", rej); });
  let id = 0; const pending = new Map();
  ws.addEventListener("message", ev => {
    const msg = JSON.parse(ev.data);
    if (msg.id && pending.has(msg.id)) {
      const {resolve, reject} = pending.get(msg.id); pending.delete(msg.id);
      msg.error ? reject(new Error(JSON.stringify(msg.error))) : resolve(msg.result);
    }
  });
  const send = (method, params={}, sessionId) => new Promise((resolve, reject) => {
    const msgId = ++id; pending.set(msgId, {resolve, reject});
    const payload = {id: msgId, method, params}; if (sessionId) payload.sessionId = sessionId;
    ws.send(JSON.stringify(payload));
  });
  const {targetId} = await send("Target.createTarget", {url: "about:blank"});
  const {sessionId} = await send("Target.attachToTarget", {targetId, flatten: true});
  await send("Page.enable", {}, sessionId);
  await send("Runtime.enable", {}, sessionId);
  await send("Page.navigate", {url}, sessionId);
  const value = await send("Runtime.evaluate", {expression: `new Promise(resolve => {
    const done = () => resolve(document.getElementById("report")?.textContent || "NO REPORT");
    if (document.body?.dataset.ready) return done();
    const obs = new MutationObserver(() => { if (document.body?.dataset.ready) { obs.disconnect(); done(); } });
    obs.observe(document.documentElement, {attributes:true, subtree:true, childList:true});
    setTimeout(done, 12000);
  })`, awaitPromise: true, returnByValue: true}, sessionId);
  if (value.exceptionDetails) { console.error(JSON.stringify(value.exceptionDetails)); process.exit(2); }
  console.log(value.result.value);
} finally { try { ws && ws.close(); } catch {} chrome.kill("SIGKILL"); }
"#,
    )
    .unwrap();

    let output = Command::new("node")
        .arg(&script)
        .arg(&chrome)
        .arg(format!("file://{}", page.display()))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "chrome check failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("OK"),
        "Plotly text overlaps:\n{stdout}\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Notebook page chrome: `.plot-container > div { height: 100% }` is the
/// rule watch and static HTML both apply around a Plotly figure.
const PAGE_HTML: &str = r#"
<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<script src="plotly.min.js"></script>
<style>
  .plot-container { background: @@BG@@; }
  .plot-container > div { width: 100%; height: 100%; }
</style>
</head>
<body style="margin:0;background:@@BG@@">
@@BODY@@
<pre id="report">pending</pre>
<script>
function roleOf(el) {
  const own = el.getAttribute("class") || "";
  const g = el.closest("g");
  const cls = own + " " + (g ? (g.getAttribute("class") || "") : "");
  if (cls.includes("annotation")) return "title";
  if (/x\d*title/.test(cls)) return "xlabel";
  if (/y\d*title/.test(cls)) return "ylabel";
  if (cls.includes("legend")) return "legend";
  if (cls.includes("colorbar") || cls.includes("cbaxis")) return "cbar";
  if (/x\d*tick/.test(cls)) return "xtick";
  if (/y\d*tick/.test(cls)) return "ytick";
  return "text";
}
function hit(a, b) {
  const x = Math.min(a.right, b.right) - Math.max(a.left, b.left);
  const y = Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top);
  return x > 1 && y > 1;
}
window.addEventListener("load", async () => {
  // Plotly.newPlot is started by each figure's own script. Wait until every
  // container has drawn at least one label, or give up after a few seconds.
  const frames = [...document.querySelectorAll(".plot-container")];
  const deadline = Date.now() + 8000;
  while (Date.now() < deadline) {
    const ready = frames.every(f => f.querySelectorAll("text").length > 0);
    if (ready) break;
    await new Promise(r => setTimeout(r, 50));
  }
  const hits = [];
  for (const frame of frames) {
    const id = frame.querySelector("[id]") ? frame.querySelector("[id]").id : "?";
    const texts = [...frame.querySelectorAll("text")].map(el => {
      const r = el.getBoundingClientRect();
      return {
        role: roleOf(el),
        text: (el.textContent || "").replace(/\s+/g, " ").trim().slice(0, 70),
        left: r.left, top: r.top, right: r.right, bottom: r.bottom,
        w: r.width, h: r.height
      };
    }).filter(t => t.text && t.w > 0 && t.h > 0);
    for (let i = 0; i < texts.length; i++) {
      for (let j = i + 1; j < texts.length; j++) {
        const a = texts[i], b = texts[j];
        if (a.role === "xtick" && b.role === "xtick") continue;
        if (a.role === "ytick" && b.role === "ytick") continue;
        if (a.role === "cbar" && b.role === "cbar") continue;
        if (hit(a, b)) hits.push(id + " " + a.role + ":" + a.text + " | " + b.role + ":" + b.text);
      }
    }
  }
  document.getElementById("report").textContent = hits.length ? hits.join("\n") : "OK";
  document.body.dataset.ready = "1";
});
</script>
</body>
</html>
"#;
