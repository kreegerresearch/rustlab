//! Widget placement: an adjacent `rustlab-widget` run is the next rustlab
//! cell's control panel. Bars, batch HTML, and `run = "release"` stay
//! covered here. See `dev/plans/notebook_widget_placement.md`.

use std::path::Path;

use rustlab_notebook::execute::execute_notebook;
use rustlab_notebook::parse::parse_notebook;
use rustlab_notebook::render::{render_html, LinkMode, WidgetsLiveGuard};
use rustlab_notebook::render_latex::render_latex;
use rustlab_notebook::render_markdown::{render_markdown, LinkStyle};
use rustlab_plot::Theme;

fn html_of(src: &str) -> String {
    let blocks = parse_notebook(src);
    let rendered = execute_notebook(&blocks);
    render_html(
        "T",
        &rendered,
        Path::new("/tmp/rustlab_widget_panel"),
        "plots",
        Theme::Dark.colors(),
        None,
        &LinkMode::single_file(),
    )
}

fn sections(html: &str) -> Vec<&str> {
    html.split("<section class=\"rl-block\"").skip(1).collect()
}

/// The panel `<div>`, not the stylesheet rule and not the inner columns.
fn panel_count(html: &str) -> usize {
    html.matches("class=\"rl-drive\"").count() + html.matches("class=\"rl-drive ").count()
}

fn has_panel(html: &str) -> bool {
    panel_count(html) > 0
}

const SLIDER: &str = "\
```rustlab-widget
name = \"amp\"
type = \"slider\"
min = 0
max = 2
default = 1
label = \"Amplitude\"
```
";

#[test]
fn adjacent_widgets_fold_into_one_drive_section() {
    let src = format!(
        "{SLIDER}\n```rustlab-widget\nname = \"freq\"\ntype = \"number\"\ndefault = 3\nlabel = \"Frequency\"\n```\n\n```rustlab\namp = widget(\"amp\");\nfreq = widget(\"freq\");\nplot(amp:freq)\n```\n"
    );
    let html = html_of(&src);
    assert_eq!(panel_count(&html), 1, "one panel:\n{html}");
    assert_eq!(sections(&html).len(), 1, "no extra widget section:\n{html}");
    let section = sections(&html)[0];
    assert!(section.contains("rl-drive-controls"), "{section}");
    assert!(section.contains("class=\"rl-drive-plot\""), "{section}");
    assert!(section.contains("data-widget-name=\"amp\""));
    assert!(section.contains("data-widget-name=\"freq\""));
    assert!(
        !section.contains("class=\"rl-widget-drives\""),
        "a panel widget does not get a drives link"
    );
}

#[test]
fn prose_between_widget_and_cell_stays_a_bar() {
    let src = format!("{SLIDER}\n## The plot\n\nA paragraph.\n\n```rustlab\nplot(widget(\"amp\") * [1, 2, 3])\n```\n");
    let html = html_of(&src);
    assert!(!has_panel(&html), "{html}");
    assert!(html.contains("class=\"rl-widget\""), "{html}");
    assert!(
        sections(&html).len() >= 2,
        "the bar keeps its own section:\n{html}"
    );
}

#[test]
fn widgets_demo_keeps_full_width_bars() {
    let src = include_str!("../../../examples/notebooks/widgets_demo.md");
    let html = html_of(src);
    assert!(!has_panel(&html), "widgets_demo must stay bars:\n{html}");
    // Option forms are `class="rl-widget rl-widget-options"`. A prefix of
    // `class="rl-widget` also matches labels and inputs.
    let forms = html.matches("class=\"rl-widget\"").count()
        + html.matches("class=\"rl-widget ").count();
    assert_eq!(forms, 3, "{html}");
    assert!(
        html.contains("href=\"#plot-1\""),
        "the plot cell reads every bar:\n{html}"
    );
    assert!(html.contains("drives: "), "{html}");
}

#[test]
fn a_second_cell_does_not_grow_another_panel() {
    let src = format!(
        "{SLIDER}\n```rustlab\nplot(widget(\"amp\") * [1, 2])\n```\n\n```rustlab\nplot(widget(\"amp\") * [3, 4])\n```\n"
    );
    let html = html_of(&src);
    let secs = sections(&html);
    assert_eq!(secs.len(), 2, "{html}");
    assert_eq!(panel_count(&html), 1);
    assert!(has_panel(secs[0]));
    assert!(!has_panel(secs[1]));
    assert!(
        !html.contains("class=\"rl-widget-drives\""),
        "a panel widget does not grow a drives link for a later reader"
    );
}

#[test]
fn mermaid_after_a_widget_is_not_a_panel() {
    let src = format!("{SLIDER}\n```mermaid\ngraph TD\n  A-->B\n```\n");
    let html = html_of(&src);
    assert!(!has_panel(&html), "{html}");
    assert!(html.contains("class=\"rl-widget\""), "{html}");
}

#[test]
fn caution_callout_ends_the_widget_run() {
    let src = "\
```rustlab-widget
name = \"before\"
type = \"slider\"
min = 0
max = 1
default = 0
label = \"Before\"
```

```rustlab-widget
not toml
```

```rustlab-widget
name = \"after\"
type = \"slider\"
min = 0
max = 1
default = 0
label = \"After\"
```

```rustlab
x = widget(\"after\");
plot([x, x])
```
";
    let html = html_of(src);
    assert!(html.contains("callout-caution"), "{html}");
    assert_eq!(panel_count(&html), 1, "{html}");
    let drive = html.split("class=\"rl-drive\"").nth(1).unwrap();
    assert!(drive.contains("data-widget-name=\"after\""), "{drive}");
    assert!(
        !drive
            .split("rl-drive-plot")
            .next()
            .unwrap()
            .contains("data-widget-name=\"before\""),
        "the widget before the callout stays a bar"
    );
    assert!(html.contains("data-widget-name=\"before\""), "{html}");
}

#[test]
fn grid_directive_tiles_inside_the_plot_column() {
    let pid = std::process::id();
    let src = format!(
        "{SLIDER}\n<!-- grid: 2 -->\n```rustlab\nx = widget(\"amp\");\nplot(1:5);\nsavefig(\"widget_grid_{pid}_a.svg\");\nfigure();\nplot(2:6);\nsavefig(\"widget_grid_{pid}_b.svg\");\n```\n"
    );
    let html = html_of(&src);
    let plot = html
        .split("class=\"rl-drive-plot\"")
        .nth(1)
        .expect("plot column");
    assert!(plot.contains("class=\"image-grid\""), "{plot}");
    assert!(
        !plot.contains("class=\"rl-widget\""),
        "the widget is not a grid cell:\n{plot}"
    );
    let _ = std::fs::remove_file(format!("widget_grid_{pid}_a.svg"));
    let _ = std::fs::remove_file(format!("widget_grid_{pid}_b.svg"));
}

#[test]
fn no_figure_cell_is_a_single_column() {
    let src = format!("{SLIDER}\n```rustlab\nprint(widget(\"amp\"))\n```\n");
    let html = html_of(&src);
    assert!(html.contains("rl-drive-solo\""), "{html}");
    assert!(!html.contains("class=\"rl-drive-plot\""), "{html}");
    assert!(!html.contains("class=\"rl-widget-drives\""), "{html}");
}

#[test]
fn batch_html_disables_controls_and_states_the_note() {
    let src = format!("{SLIDER}\n```rustlab\nplot(widget(\"amp\") * [1, 2, 3])\n```\n");
    let html = html_of(&src);
    assert!(html.contains(" disabled"), "{html}");
    assert!(
        html.contains("default values — open with notebook watch to interact"),
        "{html}"
    );
    assert!(!html.contains("widget_update"), "{html}");
    assert!(!html.contains("data-widget-run"), "{html}");
}

#[test]
fn live_render_leaves_controls_enabled() {
    let _live = WidgetsLiveGuard::enable();
    let src = format!("{SLIDER}\n```rustlab\nplot(widget(\"amp\") * [1, 2])\n```\n");
    let html = html_of(&src);
    assert!(!html.contains(" disabled"), "{html}");
    assert!(
        !html.contains("default values — open with notebook watch to interact"),
        "{html}"
    );
    assert!(has_panel(&html), "{html}");
}

#[test]
fn run_release_is_on_the_form_and_option_run_is_a_caution() {
    let src = "\
```rustlab-widget
name = \"amp\"
type = \"slider\"
min = 0
max = 2
default = 1
label = \"Amplitude\"
run = \"release\"
```

```rustlab
plot(widget(\"amp\") * [1, 2])
```
";
    let html = html_of(src);
    assert!(html.contains("data-widget-run=\"release\""), "{html}");

    let plain = html_of(&format!(
        "{SLIDER}\n```rustlab\nplot(widget(\"amp\") * [1, 2])\n```\n"
    ));
    assert!(
        !plain.contains("data-widget-run"),
        "the default slider does not set run:\n{plain}"
    );

    let bad = "\
```rustlab-widget
name = \"w\"
type = \"option\"
choices = [\"a\", \"b\"]
default = \"a\"
run = \"release\"
```
";
    let html = html_of(bad);
    assert!(html.contains("callout-caution"), "{html}");
    assert!(html.contains("only valid on a slider"), "{html}");
    assert!(!html.contains("class=\"rl-widget\""), "{html}");
}

#[test]
fn bar_links_to_the_figure_its_reader_emitted() {
    let src = "\
```rustlab
plot([1, 2, 3])
```

```rustlab-widget
name = \"k\"
type = \"slider\"
min = 0
max = 2
default = 1
label = \"Gain\"
```

This stays a bar.

```rustlab
plot(widget(\"k\") * [1, 2, 3])
```
";
    let html = html_of(src);
    assert!(!has_panel(&html), "{html}");
    assert!(
        html.contains("href=\"#plot-2\""),
        "the reader emits plot-2:\n{html}"
    );
    assert!(html.contains(">Figure 2</a>"), "{html}");
    assert!(
        !html.contains("href=\"#plot-1\""),
        "the first plot did not read the widget:\n{html}"
    );
}

#[test]
fn a_reader_with_no_figure_has_no_drives_link() {
    let src = format!("{SLIDER}\nJust a bar.\n\n```rustlab\nprint(widget(\"amp\"))\n```\n");
    let html = html_of(&src);
    assert!(!html.contains("class=\"rl-widget-drives\""), "{html}");
    assert!(!html.contains("href=\"#plot-"), "{html}");
}

#[test]
fn markdown_and_latex_stay_label_equals_value() {
    let src = format!("{SLIDER}\n```rustlab\nprint(widget(\"amp\"))\n```\n");
    let blocks = parse_notebook(&src);
    let rendered = execute_notebook(&blocks);
    let dir = Path::new("/tmp/rustlab_widget_panel");
    let md = render_markdown(
        "T",
        &rendered,
        dir,
        "plots",
        Theme::Dark.colors(),
        None,
        LinkStyle::Standard,
        false,
        "",
    );
    assert!(md.contains("**Amplitude:** `1`"), "{md}");
    assert!(!md.contains("class=\"rl-drive"), "{md}");
    let tex = render_latex(
        "T",
        &rendered,
        dir,
        "plots",
        Theme::Dark.colors(),
        &LinkMode::single_file(),
    );
    assert!(tex.contains("\\textbf{Amplitude:}"), "{tex}");
    assert!(tex.contains("\\texttt{1}"), "{tex}");
    assert!(!tex.contains("rl-drive"), "{tex}");
}
