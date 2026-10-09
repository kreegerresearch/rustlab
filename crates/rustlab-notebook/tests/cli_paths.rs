//! CLI-level path-resolution checks that need a real process with its own
//! cwd — in-process tests race on the process-global cwd with parallel
//! renders, and `cmd_render`'s error paths call `process::exit`.

use std::path::Path;
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustlab-notebook"))
}

#[test]
fn render_single_file_honours_a_relative_output_path() {
    // A relative `-o` used to resolve AFTER the chdir to the notebook's
    // parent — output landed inside the source dir while the summary
    // printed the cwd-relative path the user asked for.
    let dir = tempfile::tempdir().unwrap();
    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("note.md"), "# Note\n\nprose\n").unwrap();

    let status = bin()
        .current_dir(dir.path())
        .args(["render", "src/note.md", "-o", "out/note.html"])
        .status()
        .expect("failed to run rustlab-notebook");
    assert!(status.success(), "render exited with {status}");

    assert!(
        dir.path().join("out/note.html").is_file(),
        "output must resolve against the invoking cwd"
    );
    assert!(
        !src_dir.join("out").exists(),
        "output landed inside the source dir"
    );
}

#[test]
fn render_dir_relative_output_resolves_against_cwd() {
    // The directory form already absolutized; pin it so the two entry
    // points can't drift apart again.
    let dir = tempfile::tempdir().unwrap();
    let src_dir = dir.path().join("nb");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("a.md"), "# A\n\nprose\n").unwrap();

    let status = bin()
        .current_dir(dir.path())
        .args(["render", "nb", "-o", "site"])
        .status()
        .expect("failed to run rustlab-notebook");
    assert!(status.success(), "render exited with {status}");
    assert!(Path::new(&dir.path().join("site/a.html")).is_file());
    assert!(!src_dir.join("site").exists());
}

#[test]
fn no_comments_strips_html_and_latex_and_comments_flag_keeps_them() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("note.md");
    std::fs::write(
        &src,
        "# Note\n\nGroup delay is ==constant== %%why the phase%%.\n",
    )
    .unwrap();

    let html_on = dir.path().join("on.html");
    let status = bin()
        .args([
            "render",
            src.to_str().unwrap(),
            "-o",
            html_on.to_str().unwrap(),
        ])
        .status()
        .expect("render");
    assert!(status.success(), "{status}");
    let on = std::fs::read_to_string(&html_on).unwrap();
    assert!(on.contains("rl-cm-mark"), "{on}");
    assert!(on.contains("rl-cm-note"), "{on}");
    assert!(on.contains("constant"));
    assert!(on.contains("why the phase"));

    let html_off = dir.path().join("off.html");
    let status = bin()
        .args([
            "render",
            src.to_str().unwrap(),
            "-o",
            html_off.to_str().unwrap(),
            "--no-comments",
        ])
        .status()
        .expect("render --no-comments");
    assert!(status.success(), "{status}");
    let off = std::fs::read_to_string(&html_off).unwrap();
    assert!(!off.contains("rl-cm-mark"), "{off}");
    assert!(!off.contains("rl-cm-note"), "{off}");
    assert!(!off.contains("id=\"rl-comments\""), "{off}");
    assert!(off.contains("constant"), "{off}");
    assert!(!off.contains("why the phase"), "{off}");

    // PDF and LaTeX share the comment emitter. Default omits notes;
    // `--comments` includes `\hl` and the note. A PDF build needs TeX,
    // so the assertion is on the `.tex` that PDF is compiled from.
    let tex_off = dir.path().join("off.tex");
    let status = bin()
        .args([
            "render",
            src.to_str().unwrap(),
            "-f",
            "latex",
            "-o",
            tex_off.to_str().unwrap(),
        ])
        .status()
        .expect("render latex");
    assert!(status.success(), "{status}");
    let tex = std::fs::read_to_string(&tex_off).unwrap();
    assert!(tex.contains("constant"), "{tex}");
    assert!(!tex.contains("why the phase"), "{tex}");
    assert!(!tex.contains("\\hl{"), "{tex}");
    assert!(!tex.contains("usepackage{soul}"), "{tex}");

    let tex_on = dir.path().join("on.tex");
    let status = bin()
        .args([
            "render",
            src.to_str().unwrap(),
            "-f",
            "latex",
            "--comments",
            "-o",
            tex_on.to_str().unwrap(),
        ])
        .status()
        .expect("render latex --comments");
    assert!(status.success(), "{status}");
    let tex = std::fs::read_to_string(&tex_on).unwrap();
    assert!(tex.contains("constant"), "{tex}");
    assert!(tex.contains("why the phase"), "{tex}");
    assert!(tex.contains("\\hl{"), "{tex}");
    assert!(tex.contains("usepackage{soul}"), "{tex}");
}

#[test]
fn help_puts_render_detail_on_render_not_watch() {
    let top = bin().arg("--help").output().expect("notebook --help");
    assert!(top.status.success());
    let top_out = String::from_utf8_lossy(&top.stdout);
    assert!(
        top_out.contains("html (default), latex, pdf, markdown, json"),
        "format line should name json:\n{top_out}"
    );
    assert!(
        top_out.contains("bash, python, or text"),
        "fence line missing:\n{top_out}"
    );
    assert!(
        top_out.contains("file browser"),
        "file browser line missing:\n{top_out}"
    );
    assert!(
        top_out.contains("Latte"),
        "PDF/LaTeX theme note missing:\n{top_out}"
    );

    let render = bin()
        .args(["render", "--help"])
        .output()
        .expect("render --help");
    assert!(render.status.success());
    let render_out = String::from_utf8_lossy(&render.stdout);
    assert!(
        render_out.contains("html (default), latex, pdf, markdown, json"),
        "render format line should name json:\n{render_out}"
    );
    assert!(
        render_out.contains("bash, python, or text"),
        "render fence line missing:\n{render_out}"
    );
    assert!(
        render_out.contains("file browser"),
        "render file browser line missing:\n{render_out}"
    );
    assert!(
        render_out.contains("always Latte"),
        "render should still say PDF is Latte:\n{render_out}"
    );
    assert!(
        render_out.contains("→ analysis.html"),
        "long render description should live on render:\n{render_out}"
    );

    let watch = bin()
        .args(["watch", "--help"])
        .output()
        .expect("watch --help");
    assert!(watch.status.success());
    let watch_out = String::from_utf8_lossy(&watch.stdout);
    assert!(
        watch_out.contains("Interactive server"),
        "watch help should describe the server:\n{watch_out}"
    );
    assert!(
        watch_out.contains("file browser"),
        "directory watch should mention the file browser:\n{watch_out}"
    );
    assert!(
        !watch_out.contains("→ analysis.html"),
        "render examples must not be stacked on watch:\n{watch_out}"
    );
}
