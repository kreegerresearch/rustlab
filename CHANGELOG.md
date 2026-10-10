# Changelog

Notable user-facing changes to rustlab, newest first. Versions are the
workspace version in `Cargo.toml`; entries under a version may land across
several PRs while that version is current. **Breaking / behavior changes**
get their own subsection with migration guidance — downstream script owners
should re-validate against those entries when upgrading.

## 0.3.9

### Added
- **Notebook highlights and comments.** Prose `==highlight==` and
  `%%comment%%` (optional `#cN`, `re #cN`, `@name`, and a date) render
  as `<mark>` and margin notes in HTML, JSON, and `notebook watch`.
  A whole-line `%%` immediately before a fence comments on that cell.
  Marks inside code fences, inline code, and math stay literal.
  `--comments` / `--no-comments` (and `~/.rustlabrc` `[notebook] comments`)
  control display. HTML and watch default to on, with an in-page
  Comments checkbox. PDF and LaTeX default to off; `--comments` uses
  `\hl` and a margin note. `notebook check` warns W006–W012, including
  a missing `#cN` (W012). `watch --annotate` (implied by `--editable`)
  mounts `POST /annotate/{slug}`: loopback Host and Origin, the file
  jail, and `If-Match` against the page's source hash (409, no write,
  when the file changed). Design: `dev/plans/notebook_comments.md`.
  A per-comment reviewed/accepted flag is reserved and not implemented.
  Replies render inside the parent card. Comment ids stay on
  `data-cm-id` and the card tooltip, not in the header. Annotating
  stamps source offsets on parsed text, so headings and other block
  syntax match a page that is not annotating. Every comment card
  (inline, block, cell, and replies) sits in a right margin column
  aligned with its numbered marker. The column stays in a directory
  page with the file browser and the contents sidebar; the prose
  shrinks. Turning Comments off, or `--no-comments` on static HTML,
  drops the column. Under 800px the cards are a drawer at the bottom
  of the section. PDF `--comments` puts block and cell notes in
  `\marginpar` as well (a footnote inside a table). The annotate
  popover is a card (Name, comment, Cancel / Highlight only / Save
  comment). Ctrl or Cmd+Enter saves; Esc cancels. `render -f markdown
  --comments-style=footnotes` turns notes into GitHub footnotes and
  highlights into `<mark>`. `--comments-style=callouts` turns notes
  into `> [!note]` and leaves `==` (Obsidian's preview hides a raw
  `%%`; the callout style is how those notes stay visible there).
  `rustlab-notebook strip` writes the `--no-comments` markdown
  cleanup and leaves every code fence unchanged. `POST /annotate` returns the new source hash and rejects
  any write whose offsets fall inside a code fence (400, file
  unchanged). A cell comment is only a `%%` line above the fence.
  Highlight is not offered on a code cell. The page offers Undo for
  the last annotate write in this `watch` process; git history is
  the archive. Cards that share a line sit side by side, level with
  the marker, and the margin grows to fit them. The source pane
  narrows so it does not cover that column. PDF `--comments` uses a
  wider margin paragraph (`marginfix`, 1.7in).

### Breaking / behavior changes
- **Directory file-browser folders start collapsed.** Every folder
  disclosure is closed until you open it. The folders that contain the
  notebook on the page stay open, so the current file stays visible.
  The index has no current notebook, so every folder starts closed
  there. The root disclosure stays open. Clicking a file no longer
  reloads the page with every folder expanded; clicking a folder
  summary still opens or closes only that folder. Static HTML and
  `notebook watch` share this markup.

## 0.3.8

### Breaking / behavior changes
- **Directory `notebook watch` pages use the collection path.**
  `ch2/filters.md` is `http://127.0.0.1:<port>/n/ch2/filters` (the `.md`
  stripped). An old `/n/<stem>` URL, including a `-N` collision suffix,
  redirects to that path. Two files named `filters.md` are
  `/n/ch2/filters` and `/n/ch3/filters`. A single-file `watch` stays at
  `/n/<stem>`. Static HTML links are unchanged. WebSocket, `/raw/`, and
  `/save/` still use an internal id; the page stamps it in
  `<meta name="rl-slug">`. Migration: bookmarks of `/n/<stem>` for a
  nested notebook follow the redirect.
- **The directory file browser shows file names.** Each row is the file
  name (`filters.md`) inside its folder. The notebook title and the full
  collection-relative path are no longer stacked on the row. Listing
  rules, order, the current-page marker, and native `<details>` collapse
  are unchanged.
- **A fenced code block with no language tag renders as `text`.** It uses
  the same boxed, uncolored panel as an explicit ` ```text ` fence in
  HTML, `notebook watch`, and PDF, including the `text` label. A tag this
  renderer does not highlight (`javascript`, `sh`) is unchanged, and an
  indented code block is still not a fence.

## 0.3.7

All entries below shipped under no released version number before this
one. Note for 0.3.6 pinners: the 2026-07-11 binaries reported 0.3.6 but
already contained the `fft` change and others below; 0.3.7 is the first
version where the number and the behavior match again (see AGENTS.md
Workflow Rule 12).

### Breaking / behavior changes
- **`rustlab-notebook watch` always opens the browser** unless
  `--no-browser` is passed or `CI` is set. The old rule opened only when
  stderr was a TTY, so IDE, launcher, and piped launches never opened.
  There is no opener override; the OS default browser is used. Openers
  now cover macOS (`open`), Windows (`cmd /c start`, then PowerShell
  `Start-Process`), WSL (`wslview`, then `cmd.exe` / `explorer.exe`, then
  the Linux list), and Linux (`xdg-open`, `gio open`, `sensible-browser`,
  `x-www-browser`). Each is waited on for at most about a second, so a
  browser binary that keeps running no longer blocks the server from
  starting. Migration: scripts that run `watch` in the background and do
  not want a window pass `--no-browser`.
- **LaTeX and PDF are always Catppuccin Latte on white paper.** `-t` and
  `~/.rustlabrc` `[notebook] theme` still theme HTML and `notebook watch`.
  They no longer paint a dark PDF page. Body text is `#4c4f69`; headings
  are unnumbered and colored (H1 mauve, H2 blue, H3 teal); the title has
  no date. Source, printed output, and errors are breakable panels with
  a mauve left rule and a small `rustlab` label above the source (omitted
  when `<!-- hide -->` hides it). Callouts and exercises are cards; a
  solution is printed, not collapsed. `<!-- grid: N -->` places plots in
  a row. Mermaid figures stay with their heading. Migration: do not pass
  `-t dark` expecting a dark PDF; install a normal TeX Live (the new
  packages are `tcolorbox`, `fancyvrb`, `sectsty`, `float`, `lmodern`,
  and `xcolor`'s `table` option). Shell-escape is still not used.
- **PDF compile no longer enables TeX shell-escape.** Plot SVGs are
  converted to PDF via fixed-argv Inkscape before `pdflatex`/`tectonic`
  runs; `\includegraphics` replaces `\includesvg`/`svg.sty`. Inkscape is
  required whenever a notebook has SVG plots. Migration: install Inkscape;
  do not rely on TeX packages that need `-shell-escape`.
- **`notebook watch` checks loopback Origin and Host.** `POST /save`
  and WebSocket upgrades require an `Origin` of
  `http://127.0.0.1:<port>`, `http://localhost:<port>`, or
  `http://[::1]:<port>` (a missing `Origin` is 403). Every request
  must present a matching loopback `Host` or it is rejected. There is
  no session token. Other processes on the same machine can still
  reach the loopback port. See `docs/security.md`.
- **Notebook file I/O is jailed.** Embeds, `run` / `load` / `save` /
  `savefig` / `saveanim` / `figure("….html")` must resolve under the
  jail root or error with `path escapes notebook directory`. The root is
  the notebook's own directory for a single-file `render` / `watch`, the
  collection root for a directory (nested notebooks may read
  `../data/`), or `--jail-root <DIR>` on either command. `sub/../x` is
  fine when it stays inside; absolute paths outside the root (including
  `/tmp`) are rejected. REPL and `rustlab run` are unchanged.
- **Raw HTML in notebook prose is sanitised in HTML output.** Only
  attribute-free formatting tags (`<b>`, `<br>`, `<sub>`, `<kbd>`,
  `<details>`/`<summary>`, `<div>`, table tags, …) pass through; any tag
  with an attribute and every other tag (`<script>`, `<iframe>`,
  `<a>`, …) renders as escaped text, and HTML comments are dropped.
  One exception: an `<img>` whose only attributes are a quoted `src`
  and an optional quoted `alt` is embedded (see Fixed). Extra
  attributes, including `on*` handlers, stay escaped. `javascript:` /
  `data:` / `vbscript:` / `blob:` links are neutralised; images keep
  `data:image/*` only. Migration: write links in markdown; use
  `> [!NOTE]` callouts and the `<!-- details: -->` directive instead of
  attributed HTML. Markdown output (`-f markdown`) is unaffected.
- **Single-output `svd` returns the singular values.** `s = svd(A)`
  now binds the singular-value vector (descending) — previously it
  bound the entire `(U, σ, V)` tuple, which was unusable as a single
  value (`size(s)` errored). `[U, S, V] = svd(A)` is unchanged.
  Migration: code that relied on the tuple binding should destructure
  explicitly.
- **`fft(x)` is now length-preserving.** It returns exactly `length(x)`
  bins instead of silently zero-padding to the next power of two;
  non-power-of-two lengths use a hand-rolled Bluestein (chirp-z) transform
  over the existing radix-2 kernel, and `ifft(X)` now accepts any length
  (previously a hard error off powers of two). New optional size argument:
  `fft(x, n)` / `ifft(X, n)` zero-pad or truncate to exactly `n` first.
  Migration: the axis idiom `f = fftfreq(length(X), fs)` is now always
  correct; scripts that relied on the implicit padding must request it
  explicitly, e.g. `fft(x, 1024)`. Windowed-frame estimators (`pwelch`,
  `stft`, `spectrogram`, `waterfall`, and their streaming forms)
  intentionally keep rounding their explicit `nfft` argument up to a power
  of two, as documented.

### Added
- Notebook prose fences tagged `bash`, `python`, or `text` use the same
  panel as a rustlab cell in HTML, `notebook watch`, and PDF. `bash` and
  `python` are server-side highlighted (comments, strings, keywords,
  numbers) with the rustlab token colors and a small language label.
  `text` stays uncolored on the quieter printed-output background, which
  also covers cell stdout the markdown renderer already writes as a
  `text` fence. Other fence tags are unchanged.
- Directory `rustlab-notebook watch` and the matching static directory
  HTML show a collapsible file browser on the index and on every notebook
  page. Rows are grouped by folder and show the notebook title plus the
  collection-relative path. The in-page heading list, index breadcrumb,
  and prev/next stay. Single-file watch/render is unchanged. LaTeX and
  PDF do not get the browser. Collapse is native `<details>` (no inline
  event handlers).
- `rustlab remote <host>` (in `--features viewer` builds, which `make`
  install` produces): run rustlab on another machine with plots in the
  local `rustlab-viewer`. Checks a viewer is listening, then runs `ssh -t
  -o ExitOnForwardFailure=yes -R <remote>:<local>` with a fresh
  per-session socket name; the remote script exports `RUSTLAB_VIEWER_SOCK`
  and removes the socket when the command exits, so a stale file never
  blocks the next session and sessions never collide. No probe of the
  remote. `--print` shows the command instead of running it. `rustlab
  repl --viewer [--viewer-name NAME]` connects at startup and wins over
  `[viewer] auto_connect` / `name` in `~/.rustlabrc`; both go through one
  connect routine. The rc `[viewer] name` is ignored whenever
  `RUSTLAB_VIEWER_SOCK` is set (named sessions bypass that variable, so
  honouring it would defeat a forward); this applies to `run --plot
  viewer` too. Failed viewer connections now name the socket path they
  tried. Guide: `docs/remote-viewer.md`.
- Notebook themes: named Catppuccin builtins `mocha`, `macchiato`,
  `frappe`, `latte` via `-t` / `--theme` (aliases `dark`→mocha,
  `light`→latte) and via `~/.rustlabrc` `[notebook] theme` / `[plot]
  theme`, which accept the same names. HTML `color-scheme` follows
  background luminance so custom dark palettes work without matching the
  Mocha static; LaTeX/PDF stay Latte (see Breaking, above). Mapping
  documented in `docs/notebooks.md`.
- Notebook HTML defines `:root { --rl-*: … }` tokens from the resolved
  `ThemeColors` palette. Page CSS uses `var(--rl-…, <literal>)` so
  visuals match the pre-token colors when a variable is missing.
- Notebook `` ```rustlab `` cells are syntax-colored in HTML (including
  `notebook watch` live updates) and in LaTeX/PDF. Highlighting follows
  the rustlab lexer (`#` and `%` comments, keywords, numbers, strings,
  operators, call-like names). HTML uses the active Catppuccin theme;
  LaTeX/PDF always uses Latte (see Breaking, above).
  PDF color is `\textcolor` with escaped tokens (not `minted`). Markdown
  export still emits plain `` ```rustlab `` fences. Printed output is
  not highlighted. In HTML and `notebook watch` the source, printed
  output, and errors share one indented block with a theme-accent left
  rule. In LaTeX/PDF they are separate breakable panels; the accent
  rule is the panel's left edge and continues when a panel breaks
  across pages. In HTML and
  `notebook watch` the source alone is an open disclosure (a `rustlab`
  summary); collapsing it leaves output, errors, and plots visible.
  LaTeX/PDF always shows the source expanded. Plots and
  animations stay full width. The disclosure's initial state is open
  unless a cell `<!-- code: collapsed -->`, notebook frontmatter
  `code: collapsed`, or `~/.rustlabrc` `[notebook] code = "collapsed"`
  says otherwise (most specific wins; `<!-- code: open -->` forces
  open). `<!-- hide -->` still removes the source. An unrecognised
  value warns (`notebook check` W005, or once on stderr for the rc
  key) and falls back to the next level. `notebook watch` keeps a
  disclosure the reader has toggled; cells they have not touched pick
  up a changed directive or frontmatter on re-render. LaTeX/PDF ignore
  the setting.
- Optional user-global settings file. rustlab reads
  `$XDG_CONFIG_HOME/rustlab/config.toml` if it exists, else `~/.rustlabrc`,
  else built-in defaults. The file is declarative TOML (never executed).
  v1 keys: `[display] format`, `[plot] theme` / `default_axis`,
  `[notebook] theme` / `code`, `[repl] history_limit`, `[viewer] auto_connect` /
  `name`. Precedence: CLI flags > in-script / REPL commands > rc >
  defaults. Unknown keys warn once; invalid values abort with path + key,
  except `[notebook] code` (warns once and falls back to open).
  Example: `docs/rustlabrc.example.toml`. REPL: `help rustlabrc`.
- Security hardening for notebooks / watch / PDF / viewer (see
  `docs/security.md`): CSP on watch pages with the nonce stamped at
  render time on rustlab's own script tags (no inline event handlers
  remain in rendered pages), HTML-escaped math restore, raw-HTML
  allow-list and dangerous URL scheme stripping (index page included),
  viewer Unix socket mode `0600`, and a 32 MiB IPC frame size cap.
- `rustlab-notebook render` / `watch` gained `--jail-root <DIR>` to
  widen the notebook file-I/O jail. Inkscape conversion failures now
  report the tail of Inkscape's stderr.
- Plot color names now include `gray`/`grey` and hex `"#RRGGBB"`
  everywhere a color string is accepted (`plot(..., "color", c)`,
  `hline`/`yline`, contour/quiver/streamplot color args).
- Plot argument validation is no longer silent: an unrecognized color
  name in a dedicated color slot (`hline(y, "dashed")`,
  `plot(..., "color", "chartreuse")`) prints a one-line stderr warning
  naming the accepted colors, and `heatmap`/`imagesc`/`bar` warn once
  per call when NaN/Inf values flow into plot data (renderers already
  handled them defensively, but silently — broken figures shipped with
  no hint at render time).
- `tic` / `toc` wall-clock stopwatch (bare or with parentheses): `tic`
  starts/restarts, `toc` returns elapsed seconds without clearing so
  repeated calls take split times; `toc` before any `tic` is an error.
  Thread-local (a `tic` inside a `parmap` worker times that worker).
- `ellipke(m)` — complete elliptic integrals K(m) and E(m) via the
  arithmetic-geometric mean. Parameter convention `m = k²`; domain
  [0, 1] with the exact limits `ellipke(1) → (Inf, 1)`; elementwise
  over vectors/matrices; `[K, E] = ellipke(m)` returns both.
- `pin_dirichlet(A, b, mask_or_indices, values) → [A, b]` — enforce
  Dirichlet boundary values on a linear system: pinned rows of `A`
  become identity rows and `b` gets the pinned values, so
  `spsolve(A, b)` reproduces the boundary potential exactly. Accepts a
  grid mask (column-major, matching `ij2k`/`ijk2k` and the
  `laplacian_*` builders) or a 1-based index list; sparse or dense
  square `A` (the sparse ordering hint survives).
- `trapz(M)` / `trapz(x, M)` — trapezoidal integration now handles
  matrices per column, returning a 1×ncols row (1-D-shaped inputs keep
  returning a scalar). Double integrals are two calls:
  `trapz(ys, trapz(xs, F))`.
- `quiver(..., "normalized")` — direction-only arrow plots: every
  vector is drawn at unit length × scale.

### Changed (plotting)
- **quiver auto-scaling is outlier-robust.** The auto-scale now keys on
  the 95th percentile of the field's nonzero magnitudes (was: the single
  longest arrow) and clamps outliers to one grid cell at draw time — a
  near-singular sample (e.g. a Biot-Savart field evaluated on the wire)
  no longer shrinks every other arrow to invisibility. Uniform fields
  are unchanged. Decimate dense grids with stride indexing
  (`U(1:5:end, 1:5:end)`), documented in `docs/functions.md`.
- **"not rendered to the terminal" warnings are deferred and
  savefig-aware.** Scripted runs that render vector plots and then save
  them (`quiver(...); savefig(...)`) no longer emit stderr noise; a plot
  that never reaches a file warns once, at the end of the run (or REPL
  line), as one combined message naming the plot kinds.
- **The viewer zooms with the plain scroll wheel, and every subplot has
  a Home button.** In `rustlab-viewer`, rolling the wheel over a 2-D
  panel now zooms both axes about the pointer (it used to pan the view;
  zoom was ctrl+wheel only). Left-drag still pans. Each subplot carries
  its own **Home** button in its header — plus the `Home` key while the
  panel is hovered, and double-click — which restores the script's
  `xlim`/`ylim` when it set any and otherwise re-fits the data. 3-D
  `surf` panels get the same Home button and `Home` key as an alias for
  the existing `R` reset; their scroll zoom and shift+scroll Z-scale are
  unchanged.

### Fixed
- **Theme fonts stay readable on the surface they sit on.** Notebook
  HTML, `notebook watch` (page, sidebar, code and output panels, and
  the CodeMirror source and cell editors), and Latte-on-white PDF /
  LaTeX no longer pair a font with a background below WCAG AA 4.5:1.
  Backgrounds are unchanged. Roles that already cleared 4.5 are
  unchanged. What moved, and why:
  - Mocha and Macchiato: footer (`#585b70` → `#81859c`, `#5b6078` →
    `#898ea5`; surface2 on the page was ~2.4:1) and code comments
    (`#6c7086` → `#787c92`, `#6e738d` → `#7c8199`; overlay0 on the code
    panel was ~3.8:1).
  - Frappé: dim text (`#a5adce` → `#aab2d1`, 4.26:1 on the border),
    footer (`#626880` → `#979caf`), mauve and blue accents
    (`#ca9ee6` → `#cda4e7`, `#8caaee` → `#98b3f0`; they missed 4.5 on
    the lighter border), and comments (`#737994` → `#868ca3`). Keyword
    and function tokens stay the stock swatches — they only sit on the
    code panel, which those swatches already clear — so they no longer
    share a hex with the accents.
  - Latte, including white PDF paper: dim text (`#6c6f85` → `#56586a`),
    footer (`#9ca0b0` → `#686d82`), headings and links
    (`#8839ef` → `#7113ec`, `#1e66f5` → `#094dd3`, `#179299` → `#12747a`;
    the old link blue was ~4.3:1 on the page and ~3.2:1 on the border),
    error red (`#d20f39` → `#c60e36`; 4.46:1 on the sidebar and 4.10:1
    on the code panel), and every syntax token. Stock peach `#fe640b`
    was ~2.3:1 on the code panel and ~3.0:1 on white; it is now
    `#ad4001`. Code-panel tokens are a smaller darkening than the
    headings of the same hue, because the code panel is lighter than
    the border.
  - CodeMirror in `notebook watch` no longer uses its default dark blue
    (`#00f` / `#00c`, unreadable on a dark code panel) or yellow-gray
    brackets (`#997`, ~2.2:1 on the Latte code panel) or the yellow
    search wash (`#ffa`, which made light theme text unreadable). Those
    classes take the theme token colors; a search match is an underline
    on the code background.
  The plot viewer's egui chrome and 3D axis labels already cleared 4.5
  on their dark panels and are unchanged. Plot series colors are
  strokes, not theme fonts.
- **HTML and `notebook watch` prose figures.** Markdown images and a
  safe raw `<img src alt>` are copied into the plot directory after the
  path-jail check and referenced from that copy (`plots/<stem>/prose-N.ext`
  beside a static HTML file, `/plots/<slug>/prose-N.ext` on the watch
  server). A relative `src` used to be resolved against the page URL, so
  in watch the browser requested `/n/<slug>/dot.png` and the figure was a
  broken image. Percent-encoded names are decoded. A path outside the
  jail is a placeholder that does not repeat the outside path, and the
  file is not served. Unsafe `<img>` tags stay escaped. Watch CSP is
  unchanged (`img-src 'self' data: blob:`; script nonce and
  `'strict-dynamic'`). Remote URLs are not fetched.
- **PDF prose figures, wide tables, and in-page links.**
  Markdown images and a safe raw `<img src alt>` in prose are copied
  into the plot directory (path-jailed; png/jpg/jpeg/svg) and included
  in PDF / LaTeX, scaled to the line width. gif/webp, remote URLs, and
  missing files become a visible placeholder and a stderr warning
  instead of a broken `\includegraphics`. PDF tables use `tabularx`
  fitted to `\linewidth` so a wide grid wraps inside the margins.
  Same-page `#heading` links emit `\hyperref` only when that page has
  a matching `\hypertarget`; a fragment with no target is plain text.
  A link to another notebook's PDF still drops its fragment. Wikilink
  fragments use the same heading slug as those targets (the shared
  preprocessor, so HTML wikilink fragments change to that slug too).
- **PDF builds no longer fail on Unicode in prose.** When `pdflatex`
  rejects a character the preamble does not declare (`2ⁿ`, `Aᵀ`, `ħ`, an
  emoji), `render -f pdf` recompiles with a fallback per rejected
  character: sub/superscript and modifier letters, Greek, and common math
  and arrow symbols map to the LaTeX macro; anything else prints as
  `[U+XXXX]`. A stderr warning lists the characters and placeholders.
- **Directory-mode PDF renders continue past a failing notebook.** The
  PDF compile step used to call `exit(1)` from inside the library, so one
  bad notebook stopped the whole `render <dir> -f pdf` build with nothing
  after it produced. Each failure is now reported (with its `<stem>.log`),
  the remaining notebooks render, and the command exits 1 at the end.
- **`notebook watch` renders KaTeX math again.** The watch
  Content-Security-Policy (`script-src` nonce + `'strict-dynamic'`)
  blocks inline event handlers, so the auto-render `onload` and the
  sidebar `onclick` never ran and display math stayed as raw `\[…\]`.
  Both now run from nonce'd scripts. The invalid `connect-src` token
  `ws://[::1]:*` is gone.
- **Clicking a notebook link in `notebook watch` no longer sticks on
  "disconnected — reconnecting…".** The WebSocket client read
  `window.__RL_TOKEN` before that assignment ran, and cross-notebook
  links do not carry `?token=`, so the upgrade returned 401 and the
  retry used the same empty secret. The token is gone; loopback
  Origin and Host checks remain (see the behavior note above).
- **Zooming a viewer panel with script limits no longer snaps back.**
  The viewer re-applied a panel's `xlim`/`ylim` on every frame, so any
  zoom or pan of a panel whose script had called `plot_limits` (or
  `xlim`/`ylim`) was undone on the next repaint. Limits are now applied
  on the panel's first show, when *changed* limits arrive from the
  script, and on Home — so a live plot re-sending the same limits each
  redraw leaves the user's view alone.
- Dark-mode notebook HTML no longer leaves prose links unstyled. Body,
  callout, and exercise `<a>` tags inherit the browser default
  (`#0000EE` / visited `#551A8B`) which is invisible on Catppuccin
  Mocha (`#1e1e2e`). They now use the theme accents (`#89b4fa` /
  `#cba6f7` in dark, the Latte blues/purples in light), with
  `color-scheme` set on `<html>` so UA chrome matches the page. The
  index intro links and the `--editable` CodeMirror `.cm-link` token
  get the same treatment.
- `notebook watch` (and directory `render`) now resolve
  collection-root paths and unique basenames from nested notebooks.
  `[x](ch2/notes.md)` and `[[ch2/notes]]` written from `ch1/` used to
  stay as `.md` hrefs — the watch server has no `*.md` route, so
  clicks 404'd. Lookup is still page-relative first; `./` and `../`
  spellings do not take the fallback; two files sharing a basename
  stay unresolved rather than picking a winner. Static HTML emits the
  climbed path (`../ch2/notes.html`) for a fallback hit.
- Bare `figure()` and `histogram(v)` / `hist(v)` statements no longer
  echo their return values (a meaningless figure-handle integer above
  every plot in notebook output — churning with global figure count
  across a directory render — and a 2×n bin matrix, respectively).
  Statement-position builtin calls now carry `nargout = 0`; assigned
  forms (`h = figure()`, `b = histogram(v)`) still return their values.
  Other builtins are unaffected.
- `heatmap`, `imagesc`, `contour`/`contourf`, and `image` (grayscale /
  colormap modes) now color by **signed** value instead of magnitude.
  Previously complex-to-real collapse used |v| at ingest, so any signed
  matrix rendered wrong: `[-2, -1; 1, 3]` showed −2 at mid-scale and −1
  identical to +1, and large-magnitude negatives rendered *hot*. All
  static paths now match the live-viewer path (real part, `.re`). For
  genuinely complex inputs this means the real part is displayed —
  take `abs(Z)` explicitly to plot magnitudes. Spectrogram/scalogram dB
  displays are unchanged (magnitude there is intentional).
- Multi-panel `subplot` figures containing heatmaps (`heatmap`/`imagesc`)
  or 3-D surfaces (`surf`) now export **all** panels to SVG. Previously
  the file was finalized after the first heatmap/surface panel and every
  later panel was silently dropped (line-plot panels were unaffected;
  PNG was unaffected). Captured notebook figures had the same defect.
- `cache` is no longer a reserved word. The 0.3.6 cache statement had
  silently reserved the lowercase identifier `cache`, breaking scripts
  that use it as a variable (`cache = 5`, `function [y, cache] = f(x)`).
  It is now a soft keyword: `cache <subcommand>` / `cache "path"` /
  `cache path.rcache` still parse as cache statements, and every other
  use is an ordinary identifier. One corner changed: a bare `cache` line
  is now a variable reference (previously a parse error asking for a
  subcommand).
- `rustlab-viewer` no longer burns CPU while idle (~10% of a core on WSLg,
  where every frame goes through RDP compositing or software GL). The GUI
  previously repainted at ~60 fps around the clock to poll for socket
  messages; repaints are now event-driven — the socket listener wakes the
  GUI when a message arrives, so an idle viewer draws no frames and plots
  appear with lower latency than the old 16 ms poll.
- `T(:)` on a 3-D tensor no longer panics the interpreter — it flattens
  column-major (the `reshape`/`ijk2k` order); linear indexing `T(k)`,
  index vectors `T(I)`, and `T(end)` now work on tensors too.
- `rustlab run` exits with code 1 when the script fails to parse or
  dies on a runtime error (previously it printed the error but exited
  0, silently passing CI gates). `AudioEof`/`Interrupted` remain
  clean exits.
- `max(a, b)` / `min(a, b)` are now elementwise over any mix of scalars,
  vectors, and matrices, with the same implicit-expansion (broadcast) rules
  as `+`. Previously the two-argument form accepted only two scalars. Per
  element, a `NaN` loses to any non-`NaN` partner. `max(M1, M2)` is the
  canonical union of two 0/1 masks.
- Imaginary numeric literals: `2j`, `1.5i`, `3e8j` parse directly, so
  `z = 1 + 2j` works and printed complex values (`1+2j`) round-trip as
  input. The suffix binds to the literal, so it is immune to a variable
  named `i` or `j` shadowing the builtin constant.

### Changed
- The non-integer index error now suggests a fix: `index 2.5 is invalid
  (must be a positive integer; round a computed index explicitly with
  floor()/round(), or use integer arithmetic)`.
- `rustlab info` prints the version and pointers to `rustlab docs` /
  `rustlab docs --json` and `rustlab-notebook --help`. It no longer
  prints a short DSP feature list. `rustlab --help` stays a subcommand
  list and adds the same two pointers.

### Docs
- Agent-facing docs match the binary. `rustlab run` exits 1 on a lex,
  parse, or runtime failure, including `--profile`. `rustlab docs --json`
  records are `{name, toolbox, subcategory, brief, detail}` (349
  builtins). Toolbox names are the twelve lowercase ids (`rustlab docs
  dsp`); there is no `Plotting` toolbox. `window --plot` and `rustlab
  plot` use the plot library's terminal charts. The gallery has 40
  rendered notebooks and `examples/` has 82 scripts. `bash`, `python`,
  and `text` fences are highlighted. LaTeX and PDF stay Catppuccin Latte
  on white paper, with no `pagecolor`.
- Corrected the physical rationale for the harmonic-mean face coefficients
  in the `laplacian_eps_2d` reference (series composition of half-cell
  fluxes / O(h) accuracy, not flux-conservation).
- Added warnings near the `spsolve` and `transpose` entries: reshaping a
  complex right-hand side with postfix `'` silently conjugates it — use
  `.'` for pure reshaping.

## 0.3.6

### Breaking / behavior changes
- **Non-integer indices are now a hard error.** `v(2.5)` raises
  `index 2.5 is invalid (must be a positive integer)`; earlier versions
  silently floored fractional indices. Scripts that relied on implicit
  flooring must round explicitly — `v(floor(n/2))` — or use integer
  arithmetic, e.g. `(n + 1) / 2` for the midpoint of an odd-length `n`.
  Landed in PR #24 alongside logical-mask indexing (`v(v > 2)`) and
  element-wise comparison fixes.
