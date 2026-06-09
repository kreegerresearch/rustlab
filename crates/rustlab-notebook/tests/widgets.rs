//! Phase 1 interactive-widget integration: the functional round trip
//! `rustlab-widget` fence → widget value table → `widget(name)` builtin →
//! rendered HTML, driven by widget-value overrides the way the interactive
//! server feeds them (`execute_notebook_cancellable(.., overrides)`).
//!
//! This is socket-free on purpose: the WebSocket transport for
//! `widget_update` is exercised separately in `server_ws_smoke.rs` (which
//! needs a bound port). Here we prove the part that produces the new output
//! when a value changes.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use rustlab_notebook::execute::execute_notebook_cancellable;
use rustlab_notebook::parse::parse_notebook;
use rustlab_notebook::render::render_html;
use rustlab_plot::Theme;
use rustlab_script::WidgetValue;

const NB: &str = "\
# Widget demo

```rustlab-widget
name = \"gain\"
type = \"slider\"
min = 0
max = 10
step = 0.5
default = 2
label = \"Gain\"
```

```rustlab
y = widget(\"gain\") * 100;
disp(y)
```
";

fn render_with(overrides: Option<&BTreeMap<String, WidgetValue>>) -> String {
    let blocks = parse_notebook(NB);
    let never = Arc::new(AtomicBool::new(false));
    let rendered =
        execute_notebook_cancellable(&blocks, never, overrides).expect("render not cancelled");
    render_html(
        "Widget demo",
        &rendered,
        Path::new("/tmp"),
        "/plots",
        Theme::Dark.colors(),
        None,
    )
}

#[test]
fn slider_renders_as_range_input_at_its_default() {
    let html = render_with(None);
    // The control is emitted as a labelled range input carrying the
    // widget name and declared bounds.
    assert!(html.contains("class=\"rl-widget\""), "no widget form:\n{html}");
    assert!(html.contains("data-widget-name=\"gain\""));
    assert!(html.contains("type=\"range\""));
    assert!(html.contains("min=\"0\""));
    assert!(html.contains("max=\"10\""));
    assert!(html.contains("step=\"0.5\""));
    // Default value 2 is reflected in the input and the readout.
    assert!(html.contains("value=\"2\""), "default not reflected:\n{html}");
}

#[test]
fn default_value_drives_code_output() {
    // With no override, widget("gain") resolves to the declared default 2,
    // so the code block prints 2 * 100 = 200.
    let html = render_with(None);
    assert!(html.contains("200"), "expected default-driven output 200:\n{html}");
}

#[test]
fn override_value_drives_a_new_render() {
    // The interactive server feeds the live slider value as an override;
    // the re-render must reflect it in BOTH the control and the computed
    // output. gain = 7 → 7 * 100 = 700, and the input renders at value 7.
    let mut overrides = BTreeMap::new();
    overrides.insert("gain".to_string(), WidgetValue::Number(7.0));
    let html = render_with(Some(&overrides));

    assert!(html.contains("value=\"7\""), "slider not at override:\n{html}");
    assert!(html.contains("700"), "output didn't follow the slider:\n{html}");
    // And the stale default-driven output is gone.
    assert!(!html.contains(">200<"), "stale default output still present");
}

#[test]
fn override_for_unknown_widget_is_ignored() {
    // An override naming a widget that doesn't exist must not crash the
    // render or leak in; declared widgets keep their defaults.
    let mut overrides = BTreeMap::new();
    overrides.insert("ghost".to_string(), WidgetValue::Number(99.0));
    let html = render_with(Some(&overrides));
    assert!(html.contains("200"), "declared default should still drive output");
    assert!(!html.contains("9900"), "unknown override must not be applied");
}

#[test]
fn malformed_widget_fence_becomes_a_caution_callout() {
    let src = "# Bad\n\n```rustlab-widget\nname = \"x\"\ntype = \"slider\"\nmin = 5\nmax = 1\ndefault = 3\n```\n";
    let blocks = parse_notebook(src);
    let never = Arc::new(AtomicBool::new(false));
    let rendered = execute_notebook_cancellable(&blocks, never, None).expect("not cancelled");
    let html = render_html(
        "Bad",
        &rendered,
        Path::new("/tmp"),
        "/plots",
        Theme::Dark.colors(),
        None,
    );
    assert!(html.contains("callout-caution"), "expected a caution callout:\n{html}");
    assert!(!html.contains("class=\"rl-widget\""), "broken widget must not render a control");
}

// ── Phase 2: number + option widgets, coercion / carry-over ──────────

const NB_OPTION: &str = "\
# Option demo

```rustlab-widget
name = \"window\"
type = \"option\"
choices = [\"hamming\", \"hann\", \"blackman\"]
default = \"hamming\"
```

```rustlab
disp(widget(\"window\"))
```
";

const NB_NUMBER: &str = "\
# Number demo

```rustlab-widget
name = \"order\"
type = \"number\"
min = 1
max = 64
default = 8
```

```rustlab
disp(widget(\"order\") + 1)
```
";

fn render_src(src: &str, overrides: Option<&BTreeMap<String, WidgetValue>>) -> String {
    let blocks = parse_notebook(src);
    let never = Arc::new(AtomicBool::new(false));
    let rendered =
        execute_notebook_cancellable(&blocks, never, overrides).expect("render not cancelled");
    render_html(
        "demo",
        &rendered,
        Path::new("/tmp"),
        "/plots",
        Theme::Dark.colors(),
        None,
    )
}

#[test]
fn option_renders_radio_group_with_default_checked() {
    let html = render_src(NB_OPTION, None);
    assert!(html.contains("data-widget-type=\"option\""), "no option widget:\n{html}");
    assert!(html.contains("type=\"radio\""));
    // All three choices present.
    for c in ["hamming", "hann", "blackman"] {
        assert!(html.contains(&format!("value=\"{c}\"")), "missing choice {c}");
    }
    // Default choice is checked, and drives the code output.
    assert!(html.contains("value=\"hamming\" checked"), "default not checked:\n{html}");
    assert!(html.contains(">hamming<") || html.contains("hamming"), "default output missing");
}

#[test]
fn option_string_override_drives_output_and_selection() {
    let mut overrides = BTreeMap::new();
    overrides.insert("window".to_string(), WidgetValue::Text("blackman".into()));
    let html = render_src(NB_OPTION, Some(&overrides));
    // The picked choice is now checked, and widget("window") returns it.
    assert!(html.contains("value=\"blackman\" checked"), "override not selected:\n{html}");
    assert!(!html.contains("value=\"hamming\" checked"), "stale default still checked");
    assert!(html.contains("blackman"), "output didn't follow the choice");
}

#[test]
fn option_invalid_choice_override_falls_back_to_default() {
    // An override naming a choice that isn't declared must not be applied;
    // the widget stays at its default (carry-over reconciliation rule).
    let mut overrides = BTreeMap::new();
    overrides.insert("window".to_string(), WidgetValue::Text("kaiser".into()));
    let html = render_src(NB_OPTION, Some(&overrides));
    assert!(html.contains("value=\"hamming\" checked"), "invalid choice should reset to default");
    assert!(!html.contains("kaiser"), "invalid choice must not leak in");
}

#[test]
fn number_override_clamps_to_declared_bounds() {
    // 999 is above max=64 → clamps to 64, so output is 64 + 1 = 65.
    let mut overrides = BTreeMap::new();
    overrides.insert("order".to_string(), WidgetValue::Number(999.0));
    let html = render_src(NB_NUMBER, Some(&overrides));
    assert!(html.contains("value=\"64\""), "number not clamped to max:\n{html}");
    assert!(html.contains("65"), "clamped value didn't drive output");
}

#[test]
fn number_within_bounds_passes_through() {
    let mut overrides = BTreeMap::new();
    overrides.insert("order".to_string(), WidgetValue::Number(12.0));
    let html = render_src(NB_NUMBER, Some(&overrides));
    assert!(html.contains("value=\"12\""));
    assert!(html.contains("13"), "12 + 1 should be 13");
}

#[test]
fn wrong_typed_override_falls_back_to_default() {
    // A string override for a numeric widget can't coerce → default (8).
    let mut overrides = BTreeMap::new();
    overrides.insert("order".to_string(), WidgetValue::Text("oops".into()));
    let html = render_src(NB_NUMBER, Some(&overrides));
    assert!(html.contains("value=\"8\""), "should fall back to numeric default:\n{html}");
    assert!(html.contains("9"), "default 8 + 1 = 9");
}


// ── Phase 3: scoped re-render (widget-aware prefix cache) ────────────

use rustlab_notebook::cache::NotebookCache;
use rustlab_notebook::execute::execute_notebook_scoped;

const NB_SCOPED: &str = "\
# Scoped

```rustlab
a = 11; disp(a)
```

```rustlab-widget
name = \"gain\"
type = \"slider\"
min = 0
max = 10
default = 2
```

```rustlab
b = widget(\"gain\") * 10; disp(b)
```

```rustlab
c = b + 1; disp(c)
```
";

fn scoped(
    cache: &mut NotebookCache,
    overrides: Option<&BTreeMap<String, WidgetValue>>,
) -> rustlab_notebook::execute::ExecutionOutcome {
    let blocks = parse_notebook(NB_SCOPED);
    let never = Arc::new(AtomicBool::new(false));
    execute_notebook_scoped(&blocks, cache, never, overrides).expect("not cancelled")
}

#[test]
fn widget_change_reruns_only_from_first_reading_block() {
    let mut cache = NotebookCache::default();

    // Executable blocks: [a=11], [b=widget*10], [c=b+1] → total 3.
    let first = scoped(&mut cache, None);
    assert_eq!(first.total_blocks, 3);
    assert_eq!(first.cached_blocks, 0, "first render runs everything");

    // Re-render with the same (default) value → full cache hit.
    let same = scoped(&mut cache, None);
    assert_eq!(same.cached_blocks, 3, "identical render hits every block");

    // Change gain → only the first block (a=11, reads no widget) stays
    // cached; the widget-reading block and everything downstream re-run.
    let mut ov = BTreeMap::new();
    ov.insert("gain".to_string(), WidgetValue::Number(7.0));
    let changed = scoped(&mut cache, Some(&ov));
    assert_eq!(
        changed.cached_blocks, 1,
        "scoped re-render: blocks before the first widget() read are reused"
    );
    assert_eq!(changed.total_blocks, 3);
}

#[test]
fn scoped_rerender_produces_correct_new_values() {
    let mut cache = NotebookCache::default();
    let _ = scoped(&mut cache, None); // gain=2 → b=20, c=21

    let mut ov = BTreeMap::new();
    ov.insert("gain".to_string(), WidgetValue::Number(7.0));
    let changed = scoped(&mut cache, Some(&ov)); // gain=7 → b=70, c=71

    // Render the outcome and confirm downstream values updated even though
    // the upstream block was served from cache.
    let html = render_html(
        "Scoped",
        &changed.rendered,
        Path::new("/tmp"),
        "/plots",
        Theme::Dark.colors(),
        None,
    );
    assert!(html.contains("11"), "cached upstream block still present");
    assert!(html.contains("70"), "widget-reading block recomputed (b=70)");
    assert!(html.contains("71"), "downstream block recomputed (c=71)");
    assert!(!html.contains(">20<"), "stale b=20 must be gone");
}
