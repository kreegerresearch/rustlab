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

`rustlab-notebook` never passes `-shell-escape` (or tectonic
`-Z shell-escape`) to the PDF engine. Plot SVGs are converted to PDF with
a **fixed-argv** Inkscape invocation (`Command`, no shell) before TeX
runs; the `.tex` uses `\includegraphics` rather than `\includesvg` /
`svg.sty`. The `--format latex` output gets the same `.pdf` companions
written next to each `.svg` so the emitted `.tex` compiles as-is.

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
on the watch origin can save; a page on another origin cannot.

## H3 — HTML / XSS + CSP

- Raw HTML in markdown prose (`Event::Html` / `InlineHtml`) goes through
  an allow-list sanitiser: **attribute-free** formatting tags (`<b>`,
  `<i>`, `<br>`, `<sub>`, `<sup>`, `<kbd>`, `<details>`, `<summary>`,
  `<div>`, `<span>`, table tags, …) pass through; any tag carrying an
  attribute, and every tag off the list (`<script>`, `<iframe>`,
  `<img>`, `<a>`, `<style>`, `<object>`, …), is rendered as escaped
  text. HTML comments are dropped. This applies to notebook prose,
  callouts, and the directory `index.md` body alike.
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

## H4 — Path jail

While executing notebook code (and when expanding embeds), every path
passed to `run`, `load`, `save`, `savefig`, `saveanim`, `figure("…html")`
and `![[embed]]` must resolve under the jail root after lexical
normalisation of `..` and canonicalisation of the existing prefix
(symlinks pointing out are rejected). Escapes fail with
`path escapes notebook directory`.

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

REPL / `rustlab run` leave the jail unset (no behaviour change).

## H5 — Viewer socket + IPC limits

After binding the Unix socket, the viewer sets mode **0600**. Framed IPC
messages larger than 32 MiB are rejected without allocating the payload.

On platforms that use TCP `127.0.0.1` instead of a Unix socket, traffic
remains loopback-only with **no application-level auth** — local-user
trust applies.

## Reporting

Please open a private security report or GitHub security advisory against
the rustlab repository for new findings.
