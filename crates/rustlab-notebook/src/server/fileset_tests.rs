//! `watch a.md ch2/b.md demo.rlab` — several files served as one
//! collection rooted at their common ancestor directory.

use super::fileset::{
    default_jail_root, expand_watch_args, initial_path, resolve_watch_inputs, WatchSet,
};
use super::*;
use axum::body::{to_bytes, Body};
use rustlab_plot::Theme;
use std::path::{Path, PathBuf};
use tower::util::ServiceExt;

const PORT: u16 = 8042;

fn write(path: &Path, text: &str) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(path, text).unwrap();
}

/// `root/intro.md`, `root/ch2/filters.md`, `root/demo.rlab`, plus an
/// unlisted `root/ch2/secret.md`.
fn sample_tree() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    write(
        &root.join("intro.md"),
        "# Intro\n\nSee [filters](ch2/filters.md) and [secret](ch2/secret.md).\n",
    );
    write(&root.join("ch2/filters.md"), "# Filters\n\nlowpass.\n");
    write(&root.join("ch2/secret.md"), "# Secret\n\nnot listed.\n");
    write(&root.join("demo.rlab"), "x = 6 * 7;\ndisp(x)\n");
    (dir, root)
}

fn listed(root: &Path) -> Vec<PathBuf> {
    vec![
        root.join("intro.md"),
        root.join("ch2/filters.md"),
        root.join("demo.rlab"),
    ]
}

fn state_for(set: &WatchSet, editable: bool, jail: Option<PathBuf>) -> Arc<http::ServerState> {
    let state = build_set_state(
        set,
        Theme::Dark.colors(),
        editable,
        editable,
        true,
        "testnonce".into(),
        jail,
        false,
    )
    .unwrap();
    state
        .bind_port
        .store(PORT, std::sync::atomic::Ordering::Relaxed);
    state
}

async fn get(app: &axum::Router, uri: &str) -> (axum::http::StatusCode, String, Option<String>) {
    let res = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri(uri)
                .header(axum::http::header::HOST, format!("127.0.0.1:{PORT}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let loc = res
        .headers()
        .get(axum::http::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let body = to_bytes(res.into_body(), 1 << 22).await.unwrap();
    (status, String::from_utf8_lossy(&body).to_string(), loc)
}

async fn post(
    app: &axum::Router,
    uri: &str,
    origin: Option<&str>,
    body: &str,
) -> (axum::http::StatusCode, String) {
    let mut req = axum::http::Request::builder()
        .method("POST")
        .uri(uri)
        .header(axum::http::header::HOST, format!("127.0.0.1:{PORT}"))
        .header(axum::http::header::CONTENT_TYPE, "text/plain");
    if let Some(o) = origin {
        req = req.header(axum::http::header::ORIGIN, o);
    }
    let res = app
        .clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = to_bytes(res.into_body(), 1 << 20).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

// ── CLI input resolution ────────────────────────────────────────────

#[test]
fn one_file_and_one_directory_keep_their_modes() {
    let (_d, root) = sample_tree();
    match resolve_watch_inputs(&[root.join("intro.md")]).unwrap() {
        WatchSet::Single(p) => assert_eq!(p, root.join("intro.md")),
        _ => panic!("one file must stay single-file watch"),
    }
    match resolve_watch_inputs(&[root.join("ch2")]).unwrap() {
        WatchSet::Dir(p) => assert_eq!(p, root.join("ch2")),
        _ => panic!("one directory must stay directory watch"),
    }
}

#[test]
fn several_files_share_their_common_ancestor_in_command_line_order() {
    let (_d, root) = sample_tree();
    let inputs = vec![
        root.join("ch2/filters.md"),
        root.join("demo.rlab"),
        root.join("intro.md"),
    ];
    match resolve_watch_inputs(&inputs).unwrap() {
        WatchSet::Files { root: r, files } => {
            assert_eq!(r, root);
            assert_eq!(files, inputs, "command-line order is the listing order");
        }
        _ => panic!("several files must be a file set"),
    }
    // All in one folder: that folder is the root.
    match resolve_watch_inputs(&[root.join("ch2/filters.md"), root.join("ch2/secret.md")]).unwrap()
    {
        WatchSet::Files { root: r, .. } => assert_eq!(r, root.join("ch2")),
        _ => panic!("expected a file set"),
    }
}

#[test]
fn relative_inputs_resolve_and_duplicates_collapse() {
    let (_d, root) = sample_tree();
    let a = root.join("intro.md");
    let a_again = root.join("ch2/../intro.md");
    let set = resolve_watch_inputs(&[a.clone(), root.join("demo.rlab"), a_again]).unwrap();
    match set {
        WatchSet::Files { files, .. } => {
            assert_eq!(files, vec![a.clone(), root.join("demo.rlab")]);
        }
        _ => panic!("expected a file set"),
    }
    // The same file twice is one file: single-file watch.
    match resolve_watch_inputs(&[a.clone(), a.clone()]).unwrap() {
        WatchSet::Single(p) => assert_eq!(p, a),
        _ => panic!("a duplicate-only list is one notebook"),
    }
}

#[test]
fn missing_paths_are_all_named() {
    let (_d, root) = sample_tree();
    let err = resolve_watch_inputs(&[
        root.join("intro.md"),
        root.join("nope.md"),
        root.join("gone/also.md"),
    ])
    .unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("does not exist"), "{msg}");
    assert!(msg.contains("nope.md"), "{msg}");
    assert!(msg.contains("also.md"), "{msg}");
    let err = resolve_watch_inputs(&[root.join("nope.md")]).unwrap_err();
    assert!(format!("{err:#}").contains("does not exist"));
}

#[test]
fn a_directory_among_other_paths_is_refused() {
    let (_d, root) = sample_tree();
    let err = resolve_watch_inputs(&[root.join("ch2"), root.join("intro.md")]).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("directory"), "{msg}");
    assert!(msg.contains("alone"), "{msg}");
}

#[test]
fn only_md_and_rlab_files_join_a_set() {
    let (_d, root) = sample_tree();
    write(&root.join("notes.txt"), "plain\n");
    let err = resolve_watch_inputs(&[root.join("intro.md"), root.join("notes.txt")]).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("notes.txt"), "{msg}");
    assert!(msg.contains(".md") && msg.contains(".rlab"), "{msg}");
    // Extension case does not matter.
    write(&root.join("UPPER.MD"), "# Upper\n");
    assert!(resolve_watch_inputs(&[root.join("intro.md"), root.join("UPPER.MD")]).is_ok());
}

#[test]
fn file_set_jail_is_each_notebooks_own_directory_unless_widened() {
    let (_d, root) = sample_tree();
    let set = resolve_watch_inputs(&listed(&root)).unwrap();
    assert_eq!(default_jail_root(&set, None).unwrap(), None);
    let wide = default_jail_root(&set, Some(&root)).unwrap();
    assert_eq!(wide, Some(root.clone()));
    let dir = resolve_watch_inputs(&[root.clone()]).unwrap();
    assert_eq!(default_jail_root(&dir, None).unwrap(), Some(root.clone()));
    let single = resolve_watch_inputs(&[root.join("intro.md")]).unwrap();
    assert_eq!(default_jail_root(&single, None).unwrap(), None);
}

// ── Glob patterns ───────────────────────────────────────────────────

/// `course/` with notebooks at three depths plus things a glob must skip.
fn glob_tree() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    for (file, text) in [
        ("course/b.md", "# B\n"),
        ("course/a.md", "# A\n"),
        ("course/ch1/b.md", "# Ch1 B\n"),
        ("course/ch1/deep/c.md", "# C\n"),
        ("course/ch1/x.txt", "not a notebook\n"),
        ("course/ch1/run.rlab", "disp(1)\n"),
        ("course/.hidden/h.md", "# hidden dir\n"),
        ("course/ch1/.secret.md", "# hidden file\n"),
        ("course/target/t.md", "# build output\n"),
        ("course/node_modules/n.md", "# vendored\n"),
        ("course/.git/g.md", "# git\n"),
        ("labs/z.rlab", "disp(2)\n"),
        ("labs/a.rlab", "disp(3)\n"),
        ("labs/notes.md", "# Notes\n"),
    ] {
        write(&root.join(file), text);
    }
    std::fs::create_dir_all(root.join("course/empty")).unwrap();
    (dir, root)
}

fn s(p: &Path) -> PathBuf {
    p.to_path_buf()
}

#[test]
fn double_star_walks_every_depth_sorted_and_skips_hidden_and_junk() {
    let (_d, root) = glob_tree();
    let got = expand_watch_args(&[root.join("course/**/*.md")]).unwrap();
    assert_eq!(
        got,
        vec![
            root.join("course/a.md"),
            root.join("course/b.md"),
            root.join("course/ch1/b.md"),
            root.join("course/ch1/deep/c.md"),
        ]
    );
}

#[test]
fn matches_go_in_place_and_keep_order_between_arguments() {
    let (_d, root) = glob_tree();
    let got = expand_watch_args(&[
        root.join("labs/notes.md"),
        root.join("labs/*.rlab"),
        root.join("course/*.md"),
    ])
    .unwrap();
    assert_eq!(
        got,
        vec![
            root.join("labs/notes.md"),
            root.join("labs/a.rlab"),
            root.join("labs/z.rlab"),
            root.join("course/a.md"),
            root.join("course/b.md"),
        ]
    );
}

#[test]
fn a_glob_skips_other_file_types_and_directories() {
    let (_d, root) = glob_tree();
    // `*` matches x.txt (skipped), run.rlab, b.md, and the `deep` folder
    // (ignored) — no "directory among files" error.
    let got = expand_watch_args(&[root.join("course/ch1/*")]).unwrap();
    assert_eq!(
        got,
        vec![
            root.join("course/ch1/b.md"),
            root.join("course/ch1/run.rlab")
        ]
    );
    let set = resolve_watch_inputs(&[root.join("course/ch1/*"), root.join("course/a.md")]).unwrap();
    assert!(matches!(set, WatchSet::Files { .. }));
    // A literal non-notebook is still an error.
    let err = resolve_watch_inputs(&[root.join("course/ch1/*"), root.join("course/ch1/x.txt")])
        .unwrap_err();
    assert!(format!("{err:#}").contains("x.txt"));
}

#[test]
fn a_pattern_that_matches_nothing_is_named() {
    let (_d, root) = glob_tree();
    let pattern = root.join("course/*.rlab");
    let err = resolve_watch_inputs(&[root.join("course/a.md"), pattern.clone()]).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains(&pattern.display().to_string()), "{msg}");
    assert!(msg.contains("matched no"), "{msg}");
    // Only non-notebooks or only folders count as no match too.
    assert!(expand_watch_args(&[root.join("course/ch1/*.txt")]).is_err());
    assert!(expand_watch_args(&[root.join("course/e*")]).is_err());
    assert!(expand_watch_args(&[root.join("missing/**/*.md")]).is_err());
}

#[test]
fn glob_and_literal_duplicates_collapse_in_first_position() {
    let (_d, root) = glob_tree();
    let set = resolve_watch_inputs(&[
        root.join("course/b.md"),
        root.join("course/*.md"),
        root.join("course/**/b.md"),
    ])
    .unwrap();
    match set {
        WatchSet::Files { files, .. } => assert_eq!(
            files,
            vec![
                root.join("course/b.md"),
                root.join("course/a.md"),
                root.join("course/ch1/b.md"),
            ]
        ),
        other => panic!("{other:?}"),
    }
    // A glob that finds one file is single-file watch.
    match resolve_watch_inputs(&[root.join("labs/n*.md")]).unwrap() {
        WatchSet::Single(p) => assert_eq!(p, root.join("labs/notes.md")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_existing_path_with_bracket_characters_is_literal() {
    let (_d, root) = glob_tree();
    write(&root.join("course/a[1].md"), "# Bracket\n");
    let got = expand_watch_args(&[root.join("course/a[1].md")]).unwrap();
    assert_eq!(got, vec![s(&root.join("course/a[1].md"))]);
}

#[test]
fn hidden_names_match_only_when_the_pattern_asks_for_them() {
    let (_d, root) = glob_tree();
    let got = expand_watch_args(&[root.join("course/ch1/.*.md")]).unwrap();
    assert_eq!(got, vec![root.join("course/ch1/.secret.md")]);
    // A literal hidden folder before the pattern is fine.
    let got = expand_watch_args(&[root.join("course/.hidden/*.md")]).unwrap();
    assert_eq!(got, vec![root.join("course/.hidden/h.md")]);
}

#[cfg(unix)]
#[test]
fn double_star_does_not_follow_symlinked_folders() {
    let (_d, root) = glob_tree();
    // A loop back to the top and a link to another tree.
    std::os::unix::fs::symlink(root.join("course"), root.join("course/ch1/loop")).unwrap();
    std::os::unix::fs::symlink(root.join("labs"), root.join("course/labs-link")).unwrap();
    let got = expand_watch_args(&[root.join("course/**/*.md")]).unwrap();
    assert_eq!(got.len(), 4, "{got:?}");
    assert!(got.iter().all(|p| !p.to_string_lossy().contains("loop")));
    assert!(got
        .iter()
        .all(|p| !p.to_string_lossy().contains("labs-link")));
}

#[test]
fn relative_patterns_resolve_against_the_working_directory() {
    let (_d, root) = glob_tree();
    let _cwd = crate::CwdGuard::new();
    std::env::set_current_dir(&root).unwrap();
    let got = expand_watch_args(&[PathBuf::from("labs/*.rlab")]).unwrap();
    let canon: Vec<PathBuf> = got
        .iter()
        .map(|p| std::fs::canonicalize(p).unwrap())
        .collect();
    assert_eq!(
        canon,
        vec![root.join("labs/a.rlab"), root.join("labs/z.rlab")]
    );
}

// ── State, URLs, drawer ─────────────────────────────────────────────

#[test]
fn file_set_identities_are_collection_paths_in_cli_order() {
    let (_d, root) = sample_tree();
    let set = resolve_watch_inputs(&listed(&root)).unwrap();
    let state = state_for(&set, false, None);
    assert!(!state.single);
    assert_eq!(state.order, vec!["intro", "ch2/filters", "demo.rlab"]);
    assert_eq!(
        state.link_slugs.get("ch2/filters.md").unwrap(),
        "ch2/filters"
    );
    assert_eq!(state.link_slugs.get("demo.rlab").unwrap(), "demo.rlab");
    assert!(!state.link_slugs.contains_key("ch2/secret.md"));
    assert_eq!(initial_path(&set, &state), "/n/intro");
    let dir = resolve_watch_inputs(&[root.clone()]).unwrap();
    let dir_state = state_for(&dir, false, None);
    assert_eq!(initial_path(&dir, &dir_state), "/");
}

#[tokio::test]
async fn file_set_pages_urls_and_drawer() {
    let (_d, root) = sample_tree();
    let set = resolve_watch_inputs(&listed(&root)).unwrap();
    let app = http::router(state_for(&set, false, None));

    let (st, page, _) = get(&app, "/n/ch2/filters").await;
    assert_eq!(st, axum::http::StatusCode::OK);
    assert!(page.contains("lowpass"), "{page}");
    // Drawer: a tree rooted at the common ancestor, folder collapsed
    // unless it holds the open page, file names as rows.
    let label = root.file_name().unwrap().to_string_lossy().to_string();
    assert!(page.contains("class=\"file-browser\""), "no file tree");
    assert!(page.contains(&format!("<span class=\"fb-label\">{label}</span>")));
    assert!(page.contains("<details class=\"fb-folder\" open data-path=\"ch2\">"));
    assert!(page.contains(">filters.md<") && page.contains(">demo.rlab<"));
    assert!(!page.contains(">secret.md<"), "unlisted file in the tree");
    // Prev/next follow the command line.
    assert!(page.contains("href=\"/n/intro\""));
    assert!(page.contains("href=\"/n/demo.rlab\""));

    let (_, intro_page, _) = get(&app, "/n/intro").await;
    let index = intro_page.find("class=\"fb-link fb-current\"").unwrap();
    assert!(intro_page[index..].find(">intro.md<").unwrap() < 200);
    assert!(
        intro_page.contains("href=\"/n/ch2/filters\""),
        "link to a listed file resolves"
    );
    assert!(
        intro_page.contains("href=\"ch2/secret.md\""),
        "link to an unlisted file stays as written"
    );
    let (_, home, _) = get(&app, "/").await;
    assert!(home.contains("n/ch2/filters") && home.contains("n/demo.rlab"));

    // Old filename-only URL redirects; lookups never touch the disk.
    let (st, _, loc) = get(&app, "/n/filters").await;
    assert!(st.is_redirection(), "{st}");
    assert_eq!(loc.as_deref(), Some("/n/ch2/filters"));
    assert_eq!(
        get(&app, "/n/ch2/secret").await.0,
        axum::http::StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&app, "/raw/ch2/secret").await.0,
        axum::http::StatusCode::NOT_FOUND
    );
    assert_ne!(
        get(&app, "/n/ch2/%2E%2E/intro").await.0,
        axum::http::StatusCode::OK
    );
    assert_ne!(
        get(&app, "/raw/ch2/../ch2/secret").await.0,
        axum::http::StatusCode::OK
    );
    let (st, raw, _) = get(&app, "/raw/ch2/filters").await;
    assert_eq!(st, axum::http::StatusCode::OK);
    assert!(raw.contains("lowpass"));
}

#[tokio::test]
async fn rlab_file_is_a_read_only_script_page() {
    let (_d, root) = sample_tree();
    let set = resolve_watch_inputs(&listed(&root)).unwrap();
    let app = http::router(state_for(&set, true, None));
    let (st, page, _) = get(&app, "/n/demo.rlab").await;
    assert_eq!(st, axum::http::StatusCode::OK);
    assert!(page.contains("<title>demo.rlab"), "title is the file name");
    assert!(page.contains("42"), "script output missing");
    assert!(page.contains("disp"), "script source missing");
    let (st, raw, _) = get(&app, "/raw/demo.rlab").await;
    assert_eq!(st, axum::http::StatusCode::OK);
    assert_eq!(raw, "x = 6 * 7;\ndisp(x)\n");

    // `--editable` writes `.md` only; a script is never written.
    let origin = format!("http://127.0.0.1:{PORT}");
    let (st, _) = post(&app, "/save/demo.rlab", Some(&origin), "evil = 1\n").await;
    assert_eq!(st, axum::http::StatusCode::FORBIDDEN);
    assert_eq!(
        std::fs::read_to_string(root.join("demo.rlab")).unwrap(),
        "x = 6 * 7;\ndisp(x)\n"
    );
    // The same guard backs cell save and annotate.
    assert!(http::notebook_md_target(&root.join("demo.rlab"), None).is_err());
    assert!(
        page.contains("const CELL_EDIT = false;"),
        "script page offers cell editing"
    );
    let (_, md_page, _) = get(&app, "/n/intro").await;
    assert!(
        md_page.contains("const CELL_EDIT = true;"),
        "control: .md pages edit"
    );
    // A listed notebook in a subfolder saves (its own directory is the jail).
    let (st, msg) = post(
        &app,
        "/save/ch2/filters",
        Some(&origin),
        "# Filters\n\nedited.\n",
    )
    .await;
    assert_eq!(st, axum::http::StatusCode::OK, "{msg}");
    assert!(std::fs::read_to_string(root.join("ch2/filters.md"))
        .unwrap()
        .contains("edited"));
    // Writes still need a loopback Origin.
    let (st, _) = post(&app, "/save/intro", None, "# x\n").await;
    assert_eq!(st, axum::http::StatusCode::FORBIDDEN);
    let (st, _) = post(&app, "/save/intro", Some("http://evil.example"), "# x\n").await;
    assert_eq!(st, axum::http::StatusCode::FORBIDDEN);
    // An unlisted notebook has no save route even under --editable.
    let (st, _) = post(&app, "/save/ch2/secret", Some(&origin), "# x\n").await;
    assert_eq!(st, axum::http::StatusCode::NOT_FOUND);
    assert!(std::fs::read_to_string(root.join("ch2/secret.md"))
        .unwrap()
        .contains("not listed"));
}

#[test]
fn a_lone_rlab_file_is_a_script_page_too() {
    let (_d, root) = sample_tree();
    let set = resolve_watch_inputs(&[root.join("demo.rlab")]).unwrap();
    let state = state_for(&set, false, None);
    assert!(state.single);
    let html = futures_html(&state, "demo");
    assert!(html.contains("<title>demo.rlab"), "{html}");
    assert!(html.contains("42"), "script did not run");
}

#[test]
fn file_set_jail_blocks_writes_outside_each_notebook_directory() {
    let (_d, root) = sample_tree();
    write(
        &root.join("ch2/filters.md"),
        "# Filters\n\n```rustlab\nsave(\"../escape.csv\", [1, 2])\n```\n",
    );
    let set = resolve_watch_inputs(&listed(&root)).unwrap();
    let state = state_for(&set, false, default_jail_root(&set, None).unwrap());
    let html = futures_html(&state, "ch2/filters");
    assert!(html.contains("path escapes notebook directory"), "{html}");
    assert!(
        !root.join("escape.csv").exists(),
        "jail was the common ancestor"
    );

    // `--jail-root` at the ancestor widens it on purpose.
    let state = state_for(&set, false, default_jail_root(&set, Some(&root)).unwrap());
    let html = futures_html(&state, "ch2/filters");
    assert!(!html.contains("path escapes notebook directory"), "{html}");
    assert!(root.join("escape.csv").exists());
}

fn futures_html(state: &http::ServerState, slug: &str) -> String {
    let nb = state.notebook(slug).unwrap();
    nb.html.blocking_read().clone()
}

// ── Live reload ─────────────────────────────────────────────────────

#[tokio::test]
async fn every_listed_file_live_reloads() {
    let (_d, root) = sample_tree();
    let set = resolve_watch_inputs(&listed(&root)).unwrap();
    let theme: &'static ThemeColors = Theme::Dark.colors();
    let state = state_for(&set, false, None);
    let mut subs: Vec<_> = ["intro", "ch2/filters", "demo.rlab"]
        .iter()
        .map(|s| state.notebook(s).unwrap().broadcast.subscribe())
        .collect();
    let (_watcher, _coord) = render_loop::spawn_set(&set, theme, state.clone()).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let edits = [
        ("intro.md", "# Intro\n\nEDIT_INTRO.\n", "EDIT_INTRO"),
        (
            "ch2/filters.md",
            "# Filters\n\nEDIT_FILTERS.\n",
            "EDIT_FILTERS",
        ),
        ("demo.rlab", "disp(31337)\n", "31337"),
    ];
    for (i, (file, text, marker)) in edits.iter().enumerate() {
        std::fs::write(root.join(file), text).unwrap();
        let got = loop {
            let msg = tokio::time::timeout(std::time::Duration::from_secs(10), subs[i].recv())
                .await
                .unwrap_or_else(|_| panic!("{file}: no live reload"))
                .unwrap();
            if msg.contains(marker) {
                break msg;
            }
        };
        assert!(got.contains(marker));
    }
    // An unlisted sibling in a watched folder does not render anything.
    std::fs::write(root.join("ch2/secret.md"), "# Secret\n\nSECRET_EDIT\n").unwrap();
    for sub in subs.iter_mut() {
        if let Ok(Ok(msg)) =
            tokio::time::timeout(std::time::Duration::from_millis(500), sub.recv()).await
        {
            assert!(!msg.contains("SECRET_EDIT"), "unlisted file was served");
        }
    }
}
