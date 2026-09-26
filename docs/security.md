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
| Files next to a notebook | Path traversal via embeds (`![[…]]`), `run` / `load` / `save` / `savefig` | Notebook directory jail |
| Viewer Unix socket | Other local users attaching to a world-readable socket; oversized IPC frames | Viewer process |

**Out of scope (this release):** OS-level sandboxing of cell execution,
remote multi-user authentication, configurable jail roots
(`--allow-path`), and re-enabling TeX shell-escape.

## H1 — PDF without shell-escape

`rustlab-notebook` never passes `-shell-escape` (or tectonic
`-Z shell-escape`) to the PDF engine. Plot SVGs are converted to PDF with
a **fixed-argv** Inkscape invocation (`Command`, no shell) before TeX
runs; the `.tex` uses `\includegraphics` rather than `\includesvg` /
`svg.sty`.

If Inkscape is missing and the notebook has SVG plots, PDF/LaTeX render
fails with a clear install hint. Do not work around this by restoring
shell-escape.

## H2 — Watch server Origin + Host checks (no token)

On `rustlab-notebook watch` startup the server:

1. Binds **only** to `127.0.0.1` (unchanged).
2. Requires every request (pages, assets, raw source, save, WebSocket
   upgrade) to present a loopback `Host` for the **bound** port:
   `127.0.0.1:<port>`, `localhost:<port>`, or `[::1]:<port>`. Anything
   else is 403. This is the DNS-rebinding defense.
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

- Markdown raw HTML (`Event::Html` / `InlineHtml`) is escaped to text —
  never emitted as live markup. Dangerous URL schemes (`javascript:`,
  `data:`, …) are stripped from links/images.
- Math restored into HTML is HTML-escaped before KaTeX delimiters are
  applied (breaks script/tag breakout inside `$…$`).
- Watch-served pages send a Content-Security-Policy with
  `default-src 'self'`, nonce + `'strict-dynamic'` for scripts, and
  loopback-only `connect-src` for WebSockets.

## H4 — Path jail (notebook directory)

While executing notebook code (and when expanding embeds), paths must
canonicalize under the notebook's directory. Escapes via `..`, absolute
paths outside the root, or symlink escapes fail with
`path escapes notebook directory`. Applies to embeds, `run`, `load`,
`save`, `savefig`, `saveanim`, and `figure("…html")`.

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
