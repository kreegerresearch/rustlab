# Security model

rustlab treats **untrusted notebook content** and **localhost-only**
interactive tools as the primary threat surfaces. This document records
the hardening that ships with the toolbox (H1–H5) and what is explicitly
out of scope.

## Threat model

| Asset | Threat | Trust boundary |
|---|---|---|
| Host filesystem / shell | Malicious notebook source that reaches TeX `write18` / shell-escape during PDF compile | PDF toolchain |
| Notebook sources under `watch --editable` | Cross-site request forgery from another origin, or DNS rebinding onto the loopback port | Watch server (`127.0.0.1`) |
| Browser origin of `notebook watch` | XSS via raw HTML or math-borne markup executing in the watch page | HTML render + CSP |
| Files next to a notebook | Path traversal via embeds (`![[…]]`), `run` / `load` / `save` / `savefig` | Notebook path jail |
| Viewer Unix socket | Other local users attaching to a world-readable socket; oversized IPC frames | Viewer process |

A notebook collection (a directory passed to `render` or `watch`) is one
trust unit: every notebook in it is equally untrusted, and the jail and
sanitiser treat them alike.

**Out of scope (this release):** OS-level sandboxing of cell execution,
remote multi-user authentication, and re-enabling TeX shell-escape.

## H1 — PDF without shell-escape

`rustlab-notebook` never enables TeX shell-escape. `pdflatex` is invoked
with `-no-shell-escape` (tectonic has no shell-escape unless
`-Z shell-escape` is passed, and that flag is not passed). The check
that rejects a shell-escape argument treats `-no-shell-escape` and
`--no-shell-escape` as *not* enabling — a substring match on
`shell-escape` would flag the disabling form. Plot SVGs are converted
to PDF with a **fixed-argv** Inkscape invocation (`Command`, no shell)
before TeX runs; the `.tex` uses `\includegraphics` rather than
`\includesvg` / `svg.sty`. The `--format latex` output gets the same
`.pdf` companions written next to each `.svg` so the emitted `.tex`
compiles as-is.

`-no-shell-escape` does not stop `\input` or `\openout`. Those are
TeX primitives. A `%%` comment is paired across `$...$`, inline code,
and HTML comments, so the note body is not left in the document as
math. With comments off (the PDF default) the note is deleted. With
`--comments` the note body is escaped text inside `\marginpar` (every
`\` and `$` is a character, not a command or math shift). Refusing a
denylist of command names is not the control. A markdown link whose
scheme is `javascript:`, `data:`, `vbscript:`, or `blob:` is plain text
in the PDF, not an `\href`. Same-page `\hypertarget` ids are generated
by the renderer and are not that check.

If Inkscape is missing and the notebook has SVG plots, PDF render fails
with an install hint (LaTeX render warns and still writes the `.tex`).
A failing conversion reports the tail of Inkscape's stderr. Do not work
around either by restoring shell-escape.

## H2 — Watch server Origin + Host checks (no token)

On `rustlab-notebook watch` startup the server:

1. Binds **only** to `127.0.0.1` (unchanged).
2. Requires every request (pages, assets, raw source, save, WebSocket
   upgrade) to present a loopback `Host` for the **bound** port:
   `127.0.0.1:<port>`, `localhost:<port>`, or `[::1]:<port>`. Anything
   else is 403 with a message naming the accepted values. This is the
   DNS-rebinding defense.
3. Requires `POST /save/{slug}` and the WebSocket upgrade to present an
   `Origin` of `http://127.0.0.1:<port>`, `http://localhost:<port>`, or
   `http://[::1]:<port>`. A missing `Origin` is 403 on those paths
   (ordinary GET navigations may omit it). `save_run_block` /
   `run_block` / `widget_update` ride the already-upgraded socket.

There is **no session token**. A secret injected into every page is
readable by any local process that can GET the HTML, and it does not
stop another process on the same machine from talking to the loopback
port. That residual risk is accepted: the watch server is a
single-user localhost tool.

`--editable` still writes notebook sources back to disk. A browser tab
on the watch origin can save; a page on another origin cannot. The save
writes only that notebook's own `.md` (see H4). Opening a notebook file
such as an SVG is not a page on the watch origin: those responses use
the sandbox policy in H3, so a script inside the file cannot present a
loopback `Origin`.

Git comparison is off unless `watch --git-diff` or `[notebook] git_diff = true`.
`--diff` only chooses the default ref and does not enable git. While git
is off the server does not spawn it: no `for-each-ref`, no file-tree dots,
and a `?diff=` query is ignored. **Show diff** in the `⋯` menu is disabled,
with a hint to pass `--git-diff`. Enabling git prints a terminal warning
and a one-line menu note that git reads the repository config and should
be used only on a repository you trust.

When git is on, comparison (`?diff=`, `?base=`, `GET /git/refs`) is
read-only. The server execs dashed plumbing binaries from `git --exec-path`
(probed outside the notebook directory) and never a shell: `git-rev-parse`,
`git-cat-file` (blob bytes only), `git-ls-tree`, `git-for-each-ref`,
`git-hash-object --no-filters`, and `git-ls-files`. Name-status is computed
in-process, so `git diff` and `git show` are not run and a `diff.external`
or `textconv` driver has no command to attach to. Every child sets
`GIT_CONFIG_NOSYSTEM=1`, `GIT_TERMINAL_PROMPT=0`, `GIT_PAGER=cat`,
`GIT_OPTIONAL_LOCKS=0`, and command overrides `core.fsmonitor=false`,
`core.hooksPath=/dev/null`, `core.pager=cat`, `diff.external=` (empty),
`protocol.allow=never`, `core.sshCommand=false`, `credential.helper=`
(empty). Inherited `GIT_DIR` and `GIT_CONFIG_*` are removed. There is no
fetch. A repository that fails git's `safe.directory` ownership check is
refused; the server does not add a `safe.directory` exception. A ref is
character-checked (no `..`, no leading `-`, no `:`) and then
`rev-parse --verify --end-of-options`. The path is the notebook the server
already has, and it must stay inside the jail and the repository. There is
no new POST route. The picker script is a nonced `<script>` with no inline
event handlers.

The operator's `~/.gitconfig` is still read. The overrides above win for
those keys, including when the repository's `include.path` sets them.
Other program-naming keys are not overridden; these plumbing commands do
not consult them. The worktree walk does not follow symlinks and does not
apply `.gitignore`.

## H3 — HTML / XSS + CSP

- Raw HTML in markdown prose (`Event::Html` / `InlineHtml`) goes through
  an allow-list sanitiser: **attribute-free** formatting tags (`<b>`,
  `<i>`, `<br>`, `<sub>`, `<sup>`, `<kbd>`, `<details>`, `<summary>`,
  `<div>`, `<span>`, table tags, …) pass through; any tag carrying an
  attribute, and every tag off the list (`<script>`, `<iframe>`,
  `<img>`, `<a>`, `<style>`, `<object>`, …), is rendered as escaped
  text. HTML comments are dropped. This applies to notebook prose,
  callouts, and the directory `index.md` body alike. PDF / LaTeX is
  separate: a markdown image, or a raw `<img>` whose only attributes
  are a quoted `src` and an optional quoted `alt`, is copied into the
  plot directory after the path jail check below and included with
  `\includegraphics`. Extra attributes stay escaped text. Remote URLs
  are not fetched.
- Dangerous URL schemes (`javascript:`, `data:`, `vbscript:`, `blob:`)
  are stripped from links; images keep `data:image/*` only. The scheme
  test ignores ASCII whitespace and control characters, as browsers do.
- Math restored into HTML is HTML-escaped before KaTeX delimiters are
  applied (breaks script/tag breakout inside `$…$`). Code output and
  plot text (titles, labels) are escaped by their emitters.
- Watch-served pages send a Content-Security-Policy with
  `default-src 'self'`, nonce + `'strict-dynamic'` for scripts, and
  loopback-only `connect-src` for WebSockets (`'self'`,
  `ws://127.0.0.1:*`, `ws://localhost:*`). A `ws://[::1]:*` source is
  not valid CSP and is omitted; the listener binds `127.0.0.1`.
- The nonce is stamped **at render time** on the script tags the
  renderer and page chrome emit; served HTML is never post-processed to
  add nonces, so a `<script>` that reached the document through author
  content would not carry one.
- Pages do not use inline event handlers (`onload`, `onclick`):
  `'strict-dynamic'` blocks them even with a nonce on the surrounding
  tag. KaTeX auto-render and the sidebar toggle are ordinary `<script>`
  bodies, which receive the page nonce.
- Rustlab syntax highlighting is server-rendered escaped `<span class="syn-*">`
  text in HTML and `\textcolor` in PDF — not `minted`, and not a CDN or
  client-side highlighter. The spans are emitted by the renderer (they
  do not pass through the raw-HTML allow-list).
- `GET /plots/…` and `GET /raw/{slug}` are notebook bytes, not the HTML
  page. They send
  `Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; sandbox`
  and `X-Content-Type-Options: nosniff`. `sandbox` has no `allow-scripts`,
  so the document has an opaque origin and cannot run script or `fetch`
  this server with the watch origin. That is what stops an SVG prose
  image (`Content-Type: image/svg+xml`) whose `<script>` would otherwise
  run when the URL is opened directly, or loaded with `<object>`,
  `<embed>`, or `<iframe>`. `<img src="….svg">` does not execute SVG
  script; the renderer emits only that tag for a prose image (never an
  inline `<svg>`, `<object>`, `<embed>`, or `<iframe>`). The page CSP
  above is unchanged. Vendored `/assets` (KaTeX, Plotly, CodeMirror)
  get `nosniff` only — they are the scripts and styles the page loads,
  not notebook files, and a document CSP on them would not be the page
  policy. `Content-Disposition: attachment` is not set: a prose SVG is a
  real `<img>`, and attachment would stop the browser from drawing it.

## H4 — Path jail

While executing notebook code (and when expanding embeds), every path
passed to `run`, `load`, `save`, `savefig`, `saveanim`, `figure("…html")`,
`whos`, `sparameters` (a Touchstone path), `cache enable` (including the
default `.rustlab/cache.db`), `cache add file`, and `![[embed]]` must
resolve under the jail root after lexical
normalisation of `..` and canonicalisation of the existing prefix
(symlinks pointing out are rejected). Escapes fail with
`path escapes notebook directory`. Prose images in HTML, `notebook watch`,
and PDF (`![alt](path)` and a safe raw `<img src alt>`) use the same
check. A path outside the root is not copied, and the placeholder in the
page does not repeat that path. Watch still serves only files under its
plot tempdir (`GET /plots/…` rejects `..`). The page CSP is unchanged:
`img-src 'self' data: blob:` and `script-src` with the per-process nonce
plus `'strict-dynamic'`.

The root is:

| Invocation | Jail root |
|---|---|
| `render <file.md>` / `watch <file.md>` | the notebook's own directory |
| `render <dir>` / `watch <dir>` | the collection root, so nested notebooks can share `../data/` |
| `--jail-root <DIR>` on `render` or `watch` | that directory (must exist and contain the notebooks) |

Relative paths always resolve against the notebook's own directory (the
process cwd at execute time, captured once), not the jail root, and
`sub/../file` is allowed when the result stays inside. Absolute paths
outside the root are rejected; widen with `--jail-root` rather than
disabling the jail.

`parmap` workers are rayon threads. They do not see the calling thread's
jail on their own, so each task installs that jail (and the caller's plot
context) and restores the worker's previous values when the task ends.
A path rejected on the notebook thread is rejected inside `parmap`,
including a `parmap` nested in another. After the task, the worker does
not keep the jail.

`POST /save/{slug}` (only when `--editable`), an inline cell save, and
`POST /annotate/{slug}` (when `--annotate` or `--editable`, including
undo) write that notebook's `.md` and nothing else. The path must end in
`.md`, and resolving it through the same jail check (symlinks followed)
must land on a `.md` file inside the jail. Annotate uses that check even
when `--jail-root` is unset: the jail is then the notebook's own
directory. The read and the write both use that canonical path. The
bytes go to a temporary file in the same directory, then `rename`
replaces the canonical path. `rename` does not follow a symlink, so a
swap of the directory entry after the check cannot retarget the write
onto another file. A `.md` name that points at an SVG, or at a file
outside the jail, is refused and the target is left untouched. The route
cannot create `eval.svg` or any other non-`.md` path: the slug selects a
notebook already in the server map. In a directory watch the slug is the
collection-relative path with `.md` stripped and may contain `/`. It is
only a map key. A `..`, `.`, empty, `\`, or NUL segment is rejected
before lookup, and the slug is never joined onto the collection root.

`rustlab-notebook strip` does not follow symlinks. A symlink file or
directory named as the input is refused. A symlink inside a directory
is skipped with a warning and is not rewritten, and the walk does not
descend a symlinked directory. A regular file is kept only when its
canonical path is still under the input directory.

A save, cell save, or annotate (including undo) that *adds* an SVG
prose image is also refused, and the file is not written. The check
uses the same markdown parse as the renderer, so it sees
`![alt](fig.svg)`, `![[fig.svg]]`, a reference-style image, and a raw
`<img src="fig.svg">` (percent-decoding and a `?query` / `#fragment` do
not hide the name). `==` and `%%` are not markdown: a highlight
`==![x](eval.svg)==` or a comment that contains an image is a prose
image when that SVG was not already in the file. A reference already
in the file on disk can be saved or annotated again, and an SVG
mentioned only inside a code fence or inline code is not a prose
image. To add a new SVG diagram, put the file in the notebook
directory and add the image line on disk; the next save or annotate
that keeps that reference is accepted. Deleting the only reference is
allowed; undo of that delete is refused, because it would add the SVG
back. SVG prose images stay a feature and are still copied and shown
with `<img>`. Rejecting every save that mentions an SVG would stop
`--editable` from saving a notebook that already has a diagram.

REPL / `rustlab run` leave the jail unset (no behaviour change).

## H5 — Viewer socket + IPC limits

After binding the Unix socket, the viewer sets mode **0600**. Framed IPC
messages larger than 32 MiB are rejected without allocating the payload.

On platforms that use TCP `127.0.0.1` instead of a Unix socket, traffic
remains loopback-only with **no application-level auth** — local-user
trust applies.

`rustlab remote` (viewer-feature builds) exposes the same socket through
an SSH remote forward: sshd on the far end creates
`/tmp/rustlab-fwd-<session>.sock` owner-only (its `StreamLocalBindMask`
defaults to `0177`) and the remote `rustlab` connects to it. The 0600
invariant on that end depends on the remote sshd's configuration, and
remote root can reach the local viewer for the life of the session. The
socket name is fresh per session and the remote script removes it when
the command exits. Nothing listens on a network port.

## Reporting

Please open a private security report or GitHub security advisory against
the rustlab repository for new findings.
