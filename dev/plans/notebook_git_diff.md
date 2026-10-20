# Plan: git diff inside `notebook watch`

**Status:** Phase 1 implemented (0.3.12). Phases 2 and 3 not started.
**Current phase:** Phase 1 complete
**Created:** 2026-10-14
**Decided:** 2026-10-20 (Michael). The ten open questions below are closed. Two of them were changed after the first recommendation: the compare ref is per page, and two refs are allowed.
**Issue:** [#66](https://github.com/kreegerresearch/rustlab/issues/66) (Michael)
**Surfaces:** interactive `rustlab-notebook watch` first. Static `render --diff` is a later phase.

## Decisions (2026-10-20)

| # | Question | Decision |
|---|---|---|
| 1 | Diff button without `--diff`? | **Yes, but quiet.** One `Diff` button in the toolbar. The compare picker stays hidden until diff is on. There is **not** one server-wide ref. Each page carries `?diff=<ref>` (and optional `?base=<ref>`). Changing the picker updates that page without a restart. |
| 2 | Default ref `HEAD`? | **Yes.** `index` is explicit (`git show :path`). |
| 3 | Run the old notebook? | **No.** Working copy vs a ref keeps the live outputs and labels them "output is the working copy". |
| 4 | Inline bars or side-by-side? | **Inline bars.** |
| 5 | Always strip `%%`? | **No.** Follow the Comments checkbox. While diff is on, the checkbox reloads with `?cm=0` or `?cm=1`. |
| 6 | Word-level prose in v1? | **No.** Whole blocks, plus a line diff inside a changed code cell. |
| 7 | `render --diff` in v1? | **No.** Phase 3. |
| 8 | Two refs? | **Yes, as two query params, not a range.** Optional `from` defaults to the working copy. `?diff=main&base=feature-x` compares those two refs. The page shows the newer commit's text (by commit time; a tie, or either side `index`, shows the `diff` param) and **does not execute it**. The banner says the code was not run. A single ref that contains `..` (`main..HEAD`) is still rejected: "one ref; the page is the working copy." |
| 9 | Embeds? | **Host file only**, with the banner "embeds are not part of this diff". |
| 10 | Ghost rows for deleted files? | **No.** The browser lists files on disk. |

Also decided with those two changes:

- The picker lists local branches (current branch first, then `main`, then the rest by committer date), then recent tags and recent commits. Typing in the field filters that list (case-insensitive substring). Arrow keys move through the matches, Enter picks one, and Esc closes the list without changing the ref. Text that matches nothing is still used when it is a valid ref. The list comes from `GET /git/refs`, a read-only route. Its argv is a fixed `git for-each-ref`. No new POST.
- Prev / next also use `[` and `]` when focus is not in an input, a textarea, a select, or CodeMirror. The buttons stay, so the editor is not stolen.
- `?diff=off` forces diff off even when the process was started with `--diff`. A branch literally named `off` is listed as `refs/heads/off`.
- Stored notebook HTML stays the unstamped working-copy render. A diff query stamps a copy of that page. A live content update (`full` / `partial` / `reconcile`) while a diff query is active reloads the page. `cell_status` does not.

## Goal

While `rustlab-notebook watch` is showing a notebook, compare that file with a git revision and see the deltas on the rendered page: which blocks were added, removed, or changed.

The working copy stays the page. Git is only a read of the "before" text. Nothing in this feature writes a file, a ref, or the index.

This is not tracked changes and not CriticMarkup. `dev/plans/notebook_comments.md` keeps review notes in the `.md` itself (`==` / `%%`). Those notes stay as they are. A git diff only *displays* what git already stores.

## Recommendation

One comparison, one view, no second execution.

| Choice | MVP |
|---|---|
| What is compared | The file on disk (after) against one git object (before), default `HEAD`. Or two refs via `?diff=` and `?base=` (the newer side is shown and not executed). |
| How you turn it on | `watch --diff` or `watch --diff <ref>`, a quiet toolbar **Diff** button, then a compact `vs` / `from` picker, or `?diff=<ref>` and optional `?base=<ref>`. |
| What you see | The rendered notebook, with a bar on added, removed, and changed blocks. Prev / next jumps between those blocks. |
| Code cells | Line diff of the cell source. The output and plots underneath are the working copy, labeled as such. The old cell is not run. |
| Prose | The whole block is marked. Word-level paint inside a paragraph waits. |
| Comments | Same as the page. Comments on: a new `%%` shows up as a changed block and its margin card. Comments off: both sides are stripped before alignment, so a comment-only edit stays quiet. |
| Where the old text comes from | `git show`, fixed argv, no shell, no new crate. |
| Who shares the ref | Nobody. The ref is the page query, not a field on `ServerState`. Two tabs can compare different refs. |

Side-by-side is not part of the plan. The page already has a file browser, a table of contents, and an 18rem comment column. A second rendered copy does not fit, and it would show two copies of every plot.

## Mockup

Toolbar when diff is on, directory watch, comments on. The file browser lists names only today (`file_browser.rs`); a dot is the only addition.

```
┌ topbar: filters.md          [ ↑ 2/5 ] [ ↓ ]  Diff [on]  vs [ HEAD____ ]   [ Source ]
├────────────┬───────────────────────────────────────────────────┬─────────────┐
│ Notebooks  │                                                   │ comments    │
│ ● filters  │  prose, unchanged                                │             │
│   intro    │                                                   │             │
│ ● bode     │ ┃ added                                         │             │
│            │ ┃ The passband is 0.25.                         │             │
│            │                                                   │             │
│            │ ┃ changed · output is the working copy          │  1 why?     │
│            │ ┃ ```rustlab                                    │             │
│            │ ┃ - h = fir_lowpass(31, ...)                    │             │
│            │ ┃ + h = fir_lowpass(63, ...)                    │             │
│            │ ┃ plot(h)          ← current figure only        │             │
│            │                                                   │             │
│            │ ┃ removed                                       │             │
│            │ ┃ ## Old heading                                │             │
│            │ ┃ (old prose, not executed)                     │             │
└────────────┴───────────────────────────────────────────────────┴─────────────┘
```

`●` is a changed notebook. The bar is a 4px stripe plus a wash behind the block. Body text stays the theme text color. Green and red are the stripe and the wash, not the letters.

```mermaid
flowchart LR
  disk["working copy .md"] --> render["normal watch render"]
  git["git show REF:path"] --> old["parse old blocks, do not run"]
  disk --> parseNew["parse host blocks"]
  parseNew --> align["block LCS"]
  old --> align
  align --> mark["stamp rl-diff on sections"]
  render --> mark
  mark --> page["one HTML page"]
```

## 1. User experience

### 1.1 Turning it on

Diff is a query on the page, not a `DiffRef` stored on `ServerState`.

| Entry | Effect |
|---|---|
| `rustlab-notebook watch path` | Unchanged, plus one quiet **Diff** button. No git calls. |
| `watch path --diff` | That browser session opens with `?diff=HEAD` as the default when the query is absent. |
| `watch path --diff <ref>` | Same, default ref `<ref>`. |
| `watch path --diff index` | Default is the index (`git show :path`). |
| Toolbar **Diff** | One quiet button. Off → `?diff=off` (clears `base`). On → `?diff=HEAD` and the picker appears. |
| `vs` field | Compact text field. Typing filters the list. Enter sets `?diff=<ref>` and reloads. No restart. |
| `from` field | Same control. Empty means the working copy. A ref sets `?base=<ref>`. |
| Free text | Typed text that matches no row is still used when it passes the character check. |
| `GET /n/<slug>?diff=<ref>` | Stamps that response only. Other tabs are untouched. |
| `GET /n/<slug>?diff=<ref>&base=<other>` | Branch vs branch. The newer side's text is rendered and not executed. |
| `GET /n/<slug>?diff=off` | Forces off, even if `--diff` set a default. |
| `GET /n/<slug>` with no query | Uses the CLI default, or plain watch when `--diff` was not passed. |

`GET /git/refs` fills both fields: `git -C <root> for-each-ref --format=… refs/heads refs/tags`, sorted in Rust (current branch, then `main`, then other heads by date, then tags). Recent commits are an optional `git log -n 12`. Every name is run through the same character check before it is sent. Not a repo, or `git` missing, is HTTP 200 with an `error` field and empty lists. POST `/git/refs` is not mounted (405).

The ref field and `?diff=` / `?base=` accept `HEAD`, `HEAD~3`, `origin/main`, a hex sha, or the token `index`. They do not accept a range. `main..HEAD` in one parameter is rejected with "one ref; the page is the working copy." Two refs are two parameters, not `A..B`.

`--diff` on the non-interactive watcher (`--obsidian` or `--output`, `watch.rs`) is ignored with the same kind of warning `--editable` already prints. Diff mode is the interactive server only.

### 1.2 The view

Inline, on the rendered page, inside `<main>`.

| Alignment | What the page shows |
|---|---|
| Unchanged | Today's section. No extra class. |
| Added | Current section plus `rl-diff rl-diff-add`, a green tint, a green side bar, and a one-word label "added". |
| Removed | A section built from the **old source only**, class `rl-diff rl-diff-del`, red tint, red side bar, struck-through text, label "removed". Prose goes through the normal markdown renderer (math, highlights, margin cards). A removed code or mermaid block is source in a `<pre>`, not an executed cell and not an SVG. |
| Changed prose | Current section plus `rl-diff rl-diff-chg`, an amber tint, an amber side bar, and the label "changed". |
| Changed code | Current section, same amber block. The line diff tints added lines green and removed lines red, with `+` / `-` in the gutter. Output, errors, and plots stay the working-copy render, with the line "output is the working copy". |
| Changed mermaid | Current SVG (one render) plus the label. Old diagram source is not drawn as a second picture. |

Removed blocks are inserted in `<main>` at the aligned position, each wrapped in `<section class="rl-block rl-diff rl-diff-del">` so the existing live-reload walker (`server/diff.rs`) still sees a flat list of sections. They are not in the working file, so they have no `data-code-idx` and no Run / Edit controls.

### 1.3 Prev / next

Two toolbar buttons and a counter (`2/5`). They call `scrollIntoView` on `section.rl-diff` inside `<main>`. `[` and `]` do the same when focus is not in an input, a textarea, a select, or CodeMirror. A block that is already fully on screen is skipped in that direction.

### 1.4 Sidebar

Directory watch only. Single-file watch has no file browser; the toolbar counter is the marker.

`FileEntry` does not gain a field. Dots are painted by the page script from a hidden `<div id="rl-git-files">` inside `<main>`, so `chrome_fingerprint` stays stable. The dot uses the add-bar color and a `title` (`modified`, `added`, `renamed`, `copied`, `type changed`). Deleted files have no row, because the browser lists files on disk.

Status comes from one `git diff --name-status -M` (working copy vs the ref, or the two refs). Untracked files use `git ls-files -z --others --exclude-standard`. The JSON is part of the stamped page, not a separate broadcast.

### 1.5 Source pane

Unchanged in the MVP. **Source** still shows the working copy via `GET /raw/{slug}`. A unified source diff in that pane is phase 2.

### 1.6 The ref is the page, not the process

`Notebook.html` stays the unstamped working-copy render. `GET /n/<slug>?diff=&base=` stamps a copy for that response. Two tabs can show two comparisons. A live content frame (`full`, `partial`, `reconcile`) while the query is on reloads the page, because removed sections would desync a position-based patch. `cell_status` does not reload. Turning diff off is `?diff=off` on that page only.

## 2. Granularity

### 2.1 What is aligned

Alignment runs on the **host file**, before `embed::expand_embeds`.

1. Read the working copy.
2. Read the old blob (`git show`).
3. If the page's comment toggle is off, run both strings through `comments::strip_source`.
4. `parse_notebook` each string. Do not expand embeds. Do not execute.
5. Fingerprint each block: kind + body. Kind is markdown, code, mermaid, widget, callout, exercise, solution. Body is the source (code and mermaid) or the markdown text. Directives (`<!-- hide -->`, `<!-- code: collapsed -->`) are part of the code fingerprint so a hide/show edit counts as a change.
6. Longest common subsequence on those fingerprints. Notebooks are short; a hand-rolled LCS is enough. No diff crate.

The displayed page is still the normal render: embeds expanded, code executed, `NotebookCache` used. After that render, walk `section.rl-block` in order and attach the LCS class. Embed expansion inserts blocks the host file does not have. Those inserted sections are left unmarked, and the toolbar says "embeds are not part of this diff" when `embed::find_embed_refs_in_line` hits the host. Phase 2 can do better. The MVP does not try to map an embedded file's blocks onto the host page.

`server/diff.rs` is a different diff. It compares two renders of the same working copy so a save can patch the DOM. It stays as it is. Git alignment is a new helper next to parse, not a change to the position-based HTML diff.

### 2.2 Word-level prose

Not in the MVP. A word diff has to survive math (`protect_math`), `==` / `%%`, inline code, and links. Painting `<ins>` inside HTML after KaTeX conversion breaks that pipeline. Doing it on markdown source before render is the right layer, and it is still easy to get wrong on a list or a table.

Phase 2 paints words only when both sides of a matched markdown block have the same block structure (same headings, lists, and tables). Anything else stays a whole-block mark. Math spans are opaque tokens: a change inside `$...$` marks that whole span, and the span is still rendered by the existing math path.

### 2.3 Code outputs and plots

Default: **do not run the old notebook.**

| | Old side | New side |
|---|---|---|
| Code source | Line diff against the new source | Line diff, in the source disclosure |
| Printed output, errors | Omitted | Working-copy output when one side is the working copy. When both sides are refs, neither side is run and the page says "code was not run". |
| Plots (Plotly, SVG, GIF) | Omitted | Working-copy figures, or none when both sides are refs |
| Mermaid SVG | Omitted | Current SVG, one `mermaid` render |
| Widgets | Old TOML shown as text if the block was removed | Current control |

Why: a second execution doubles the cost, uses a second evaluator and a second RNG stream, can touch the jail (`load`, `save`, `run`), and writes plots. `NotebookCache` stores one prefix for the working copy (`cache.rs`); it is not a map from git oid to output. Re-running the old file would be a second cache with snapshot clones of the evaluator. That is the expensive design, and the useful delta in a notebook review is the source.

The line "output is the working copy" is the honest label on a changed code cell. An unchanged code cell has no label and no extra text.

### 2.4 Caching and cost

| Data | Cache | Invalidation |
|---|---|---|
| Old blob text | In memory, keyed by `(repo, ref, relative path)` → git oid → bytes | Ref change, or oid change on the next `git rev-parse` |
| Old parse | Derived from that blob. Not stored in `NotebookCache`. | With the blob |
| Working copy | Existing `NotebookCache` | Unchanged |
| Name-status map | Computed while stamping the page. Not stored on `ServerState`. | With the page |

Turning diff on does not clear the execution cache. The render that follows is a normal working-copy render plus a classify step. A prose-only save still costs no re-execution.

A file larger than 512 KiB, or a parse with more than 400 blocks, skips inline marks. The page renders normally and the toolbar says the diff was too large, with the ref field still available. The phase 2 source pane is the place for that file.

## 3. Reading the old content

Use the `git` binary through `std::process::Command`. No shell. No `gix`, no `git2`.

| Call | Argv |
|---|---|
| Repo root, once | `git -C <start> rev-parse --show-toplevel` |
| Ref check | `git -C <root> rev-parse --verify --end-of-options <ref>^{commit}` |
| Old file | `git -C <root> show --no-textconv --end-of-options <rev>:<relpath>` |
| Sidebar | `git -C <root> diff --name-status -M --end-of-options <rev>` or `<base> <shown>` |
| Index file | `git -C <root> show --no-textconv --end-of-options :<relpath>` |
| Branch list | `git -C <root> for-each-ref --format=%(refname)%00%(HEAD)%00%(refname:short)%00%(committerdate:unix) refs/heads refs/tags` |
| Recent commits | `git -C <root> log -n 12 --format=%H%x09%h%x09%s --end-of-options` |
| Commit time | `git -C <root> log -1 --format=%ct --end-of-options <verified>` |
| Untracked | `git -C <root> ls-files -z --others --exclude-standard` |

`<start>` is the notebook's directory (single file) or the watched directory. `<relpath>` is computed from the notebook's real path and the toplevel. It is never taken from the URL.

Trade-offs:

| | `git` subprocess | `gix` or `git2` |
|---|---|---|
| Code | A few fixed argv lists and a ref checker | A library, object-store reads, rename detection |
| Dependencies | None. Git is already required for the feature to mean anything. | New crate, supply chain, build time. Neither is in the workspace today. |
| Behavior | Matches the `git` the user runs, including `HEAD`, `HEAD~3`, and worktrees | Close, but not the same binary, and easy to drift |
| Failure | "git not found" is a clear banner | A compile-time dependency for a feature many watch sessions will not use |

Recommendation: the subprocess. Revisit a crate only if Windows argv or a missing `git` becomes a real problem. Even then, prefer documenting "install git" over vendoring a git implementation. This is infrastructure, not a numeric algorithm, but a dependency is still not needed.

`--end-of-options` stops a ref that looks like a flag. `--no-textconv` stops a repo `.gitattributes` filter from running a converter over the blob.

## 4. Comments, math, mermaid

### 4.1 Comments

Show them when the page shows them.

- Comments on (`CommentMode.show`, the watch default): alignment sees `==` and `%%`. A new note changes that block's fingerprint, so the block is "changed" and the margin card renders through the existing comment path. The card is the delta. Do not invent a second comment style for diff.
- Comments off (the checkbox, or `--no-comments`): `strip_source` on both sides first, then align. A comment-only edit does not mark the block, because the reader cannot see the note anyway. Highlights are stripped by the same function; that matches the checkbox, which clears highlight backgrounds too.
- No third switch. The existing Comments checkbox is the switch.
- Annotate (`--annotate` / `--editable`) still edits the working copy. A margin-card edit that saves goes through today's `POST /annotate`, then the diff re-runs. Diff mode does not add edit or delete controls of its own.

`%%` inside a code fence is an rlab comment, not a review note (`comments.rs`). It is part of the cell source and shows up in the line diff. That is correct.

### 4.2 Math

Alignment hashes the markdown source, so a math edit marks the block. The visible math is produced only by `protect_math` / `restore_math` and the page's `\(` `\[` KaTeX. The diff pass does not scan HTML for `$`, does not wrap a math span in `<ins>` in the MVP, and does not add a KaTeX plugin. Phase 2 treats a `$...$` / `$$...$$` span as one token.

### 4.3 Mermaid

Compare the fenced source. The SVG on the page is the working copy, from the existing mermaid cache. A removed diagram is the fenced source in `<pre class="mermaid-source">`, which the renderer already uses as a fallback. No second SVG, no CDN.

## 5. Live updates

The stored HTML is not stamped. `classify` still compares working-copy renders. A browser that has `?diff=` set reloads on `full` / `partial` / `reconcile` so the stamped copy, including removed sections and file dots, is rebuilt. `cell_status` is left alone. File dots are not a `git_status` frame and are not written into the file-browser HTML.

The read-only Source pane still refreshes from `/raw/` on `__rlAfterUpdate`. Diff mode does not change that.

Turning the toggle off re-renders once without stamping, then the page is today's watch.

## 6. Security

Fits the current model (`docs/security.md`, `server/auth.rs`). No new trust boundary.

| Rule | How |
|---|---|
| Read-only git | The only subcommands are `rev-parse`, `show`, `diff`, `for-each-ref`, `log`, and `ls-files`. No `checkout`, `apply`, `add`, `commit`, `clean`, or `config`. |
| No shell | `Command::new("git")` with an argument vector. |
| Ref characters | Allow `A–Z a–z 0–9 . _ / ^ ~ @ { } -` only. Reject empty, longer than 256 bytes, a leading `-`, `:`, whitespace, `\`, and any other byte. `:` is rejected so a ref cannot smuggle `ref:otherpath` into `git show`. |
| Then ask git | `rev-parse --verify --end-of-options <ref>^{commit}` must succeed. A ref that passes the character check and is not a commit is still rejected. `index` skips this and uses the `:` stage. |
| Path | Relative path from toplevel to `Notebook.source_path`. Reject `..`, absolute paths, and anything outside the toplevel or the jail. The URL slug is resolved to a notebook the server already has, then the path is taken from that notebook. |
| Routes | No new `POST`. No new write. `?diff=` and `?base=` are queries on the existing `GET /n/...`. `GET /git/refs` is read-only. Loopback `Host` already covers every request. Origin checks on `POST /save`, `POST /annotate`, and the WebSocket stay as they are. CSP nonces are unchanged; the picker script is a nonced `<script>` with no inline `on*` handlers. |
| CSP | Diff markup is rendered by the server into the page. No inline handlers. Styles use the existing nonce-free stylesheet and `--rl-*` tokens. No CDN. `connect-src` is unchanged. |
| User files | `/raw/` and `/plots/` keep `USER_FILE_CSP` and `nosniff`. Git output is not served from those routes. |
| Jail | `git -C` is the repo root only when that root contains the watched directory. A `--jail-root` that is wider than the repo still does not let `git show` read a path outside the repo. A notebook path outside the jail is not a notebook the server renders. |
| Textconv | `--no-textconv` so a hostile `.gitattributes` filter does not run. |
| Huge output | Cap the blob read (the 512 KiB skip). Drop stderr from git except a short banner. |

`git show` of a blob the user already has in `.git` is the same trust as reading the working tree, which watch already does.

## 7. Edge cases

| Case | Behavior |
|---|---|
| Not a git repo | Watch works. **Diff** is disabled. Toolbar: "not a git repository". |
| `git` missing | Same, with "git not found". |
| Bad ref | Page stays on the previous good ref, or on plain watch if there was none. Toolbar: "unknown ref". The notebook still loads. |
| Untracked file | Every host block is "added". No `git show`. |
| Clean file | Toolbar: "no changes". No bars. |
| Renamed | `git diff -M --name-status` reports `R`. `git show` uses the old path. The row tooltip says "renamed". |
| Deleted from disk | No page and no browser row. Not in the MVP. |
| Deleted block inside a file | A "removed" section, as in §1.2. |
| Binary / NUL in the blob | Skip inline diff. Toolbar: "not a text notebook". |
| Huge file | §2.4 cap. Normal render, no bars. |
| Embeds | Host file only. Banner in §2.1. |
| Merge conflict markers | They are ordinary lines. If they are in the working copy, they render as text and the block shows as changed. No special conflict UI. |
| Submodules, sparse checkout | If `git show` fails, the toolbar shows the failure and the page is plain watch. |
| `--editable` at the same time | Allowed. Saves write the working copy as today, then the diff refreshes. Diff mode adds no save path. |
| Two refs | Allowed as `?diff=<ref>&base=<ref>`. The newer side is shown and not executed. A range string in one parameter is still rejected. |
| Interactive watcher vs `--obsidian` | `--diff` ignored on the disk re-renderer, with a warning. |

## 8. Colors

Four new `ThemeColors` fields, emitted as `--rl-diff-add-bg`, `--rl-diff-add-bar`, `--rl-diff-del-bg`, `--rl-diff-del-bar`, next to the comment tokens. All four themes get them. Latte is the light one; mocha, macchiato, and frappé are dark. PDF / LaTeX do not use these until phase 3, and PDF stays Latte on white when they do.

Rules, same for every theme:

- Block text uses `--rl-text`. It is not green or red.
- The wash is a low blend of Catppuccin green or red over that theme's `--rl-bg`, the same way `cm_mark_bg` blends yellow. `--rl-text` on the wash must be at least 4.5:1 (`theme::contrast_ratio`).
- The 4px bar is the theme green or red at full strength. It is non-text: at least 3:1 against `--rl-bg`.
- Do not reuse `error_text` as a fill. On Latte, stock green `#40a02b` is about 2.5:1 on the page background, so it cannot be the text color. The bar can use a darkened green if 3:1 fails.
- A removed block is not faded with opacity. Opacity drops the text contrast. The wash, the bar, strikethrough on the "removed" label only, and the label text are the signals.
- The file-browser dot uses the "changed" bar color and a `title`, so it is not color alone.
- An added or removed line inside a code disclosure uses the same wash on that line. The line's code text keeps the syntax colors, which already clear 4.5:1 on `--rl-code-bg`. If a wash would put a syntax color under 4.5:1, the line wash goes on the margin of the line (the bar) and the line background stays `--rl-code-bg`.

Tests pin the four ratios on mocha, macchiato, frappé, and latte. A theme that fails does not ship with a hardcoded fallback color.

## 9. Phases

Each phase is shippable on its own. Do not start phase 2 in the same change as phase 1.

### Phase 1 — MVP

- `watch --diff` and `watch --diff <ref>`, including `index`. The flag is a default for pages that omit `diff`.
- Quiet toolbar **Diff** button. The `vs` / `from` picker is hidden until diff is on.
- `?diff=<ref>` and optional `?base=<ref>` stamp that response only. `GET /git/refs` fills the lists.
- Block LCS, inline bars, removed sections from old source, line diff inside a changed code cell. Working copy vs a ref keeps live outputs. Two refs render the newer text and do not run it.
- Sidebar dots and the `git_status` frame.
- `git` subprocess, ref check, path check, size cap.
- Comment toggle behavior in §4.1.
- Four theme tokens and contrast tests.
- Banners for not-a-repo, untracked, rename, embeds, bad ref, too large.

Watch with no `--diff` is the same HTML as today, aside from the quiet **Diff** button. The picker stays hidden until that button is on.

### Phase 2 — finer text

- Word-level paint inside a changed markdown block, under the structure rule in §2.2. Math spans stay one token.
- Source pane: a read-only unified diff (`+` / `-` lines) when Diff is on, fetched from a `GET` the server already authorizes. Not CodeMirror merge view.
- The 512 KiB fallback uses that source view.

### Phase 3 — static output

- `rustlab-notebook render --diff <ref> -o out.html`. Same classifier and the same CSS tokens. No WebSocket, no toolbar. One self-contained HTML file.
- PDF only after HTML works: a left rule on the Latte paper, text color unchanged. If the LaTeX change is awkward, ship HTML and stop.

Not scheduled, and not recommended unless phase 1 feels insufficient:

- Side-by-side rendered panes.
- Running the old notebook to diff plots.
- A `gix` / `git2` dependency.
- A range syntax (`A..B`) inside one ref parameter. Two refs already ship as `?diff=` and `?base=`.
- Ghost rows for files deleted from disk.
- Diff on the `--obsidian` disk watcher.

## 10. Tests

Phase 1, in the same change as the code. Headless, no browser, no network.

| Test | What it pins |
|---|---|
| Ref checker | Accepts `HEAD`, `HEAD~2`, `origin/main`, a 40-hex sha, `index`. Rejects `HEAD;id`, `-rf`, `HEAD:../../x`, `main..HEAD`, a newline, a 300-byte string. |
| Argv builder | The `show` / `diff` / `rev-parse` vectors contain `--end-of-options` and never a shell metacharacter. Path argument equals the computed relative path. |
| Block LCS | Insert, delete, substitute, and a swap of two blocks. Code directive change is a substitute. |
| Comment strip | With comments off, a `%%`-only edit aligns as unchanged. With comments on, it is a change. |
| Render classes | A fixture notebook yields `rl-diff-add`, `rl-diff-del`, `rl-diff-chg` on the right sections. A changed code section contains the new output and both source lines. A removed code section contains no plot markup. |
| Themes | `contrast_ratio` of `--rl-text` on both washes ≥ 4.5, and each bar against `--rl-bg` ≥ 3, for all four palettes. |
| Git fixture | Temp repo: clean, modified, untracked, renamed (`-M`), not a repo, missing blob. Uses the real `git` if present; skips with a reason if it is not, except the argv and ref-checker tests, which do not need `git`. |
| Router | Diff on does not mount a route that was not already mounted. No new POST. |
| Jail | A slug that is not one of the server's notebooks does not produce a `git show` path. |
| Live HTML | After a one-block edit, `classify` is still `Partial` when diff classes are present on both sides. |
| Size cap | Over the cap, the HTML has no `rl-diff` section. |

Phase 2 adds a word-diff fixture with `$x$` left intact as one span, and a source-pane HTML fixture that contains no `<script>` from the diff text.

Phase 3 adds one static HTML render of a fixture and checks the classes and the absence of a CDN script.

## 11. Implementation notes

When phase 1 is built, the same change should:

- Bump the workspace version (rule 12). Phase 1 lands as 0.3.12. New CLI behavior and, when diff is on, different HTML are observable.
- Document the flag and the toolbar in `docs/notebooks.md`, and the read-only git calls in `docs/security.md`.
- Add a `CHANGELOG.md` entry under Added.
- Mention the flag in `docs/agent-guide.md` where watch flags are listed.
- Add a row to the Active Plans table in `AGENTS.md` while the work is in progress.

No new builtin, so `docs/functions.md`, `docs/quickref.md`, and REPL help stay as they are.

Likely files, phase 1: `main.rs` (clap), `server/mod.rs`, `server/http.rs`, `server/page.rs`, `server/ws.rs`, `server/render_loop.rs`, `file_browser.rs`, `render.rs` or a new `gitdiff.rs` for align-and-stamp, `theme.rs`, tests next to those. `execute.rs` and `cache.rs` should not need a second execution path.

## 12. Open questions — decided

Closed on 2026-10-20. The table at the top of this file is the record. Questions 1 and 8 were changed from the first recommendation: the button is quiet and the ref is per page, and two refs are allowed through `base` (the newer side is not executed). A range string in one ref is still rejected.
