# Request: slugified heading ids so cross-notebook fragments resolve

Affects the `fix/notebook-navigation` branch (PR #39) and earlier.

## Symptom

A cross-notebook link with a fragment to a *plain* heading survives link
resolution but lands nowhere:

```markdown
[details](02-filter.md#section-two)   <!-- 02-filter.md has `## Section Two` -->
```

The HTML resolver keeps the fragment (`02-filter.html#section-two`,
`/n/02-filter#section-two`), but generated heading ids are `heading-N` —
nothing on the target page matches, so the browser opens the page at the
top. Only headings written with an explicit `{#anchor}` are stable targets.
The PDF resolver drops fragments for exactly this reason.

Pinned by `cross_notebook_fragment_needs_an_explicit_anchor` in `lib.rs`
tests — delete that pin when implementing this.

## Proposal

Emit GitHub-style slug ids (`section-two`) for headings without an explicit
anchor, with `-N` dedup for repeats, keeping `heading-N` as an additional
fallback if needed for the live-reload diffing (block hashes include the
ids, so id-scheme changes force full refreshes on old clients once).

Considerations:

- `heading-N` ids are load-bearing in the WS client (TOC hrefs) and the
  chrome fingerprint; slugs are strictly more stable under edits — a
  heading INSERT today renumbers every later id, which is why partial
  updates need the chrome-upgrade path at all.
- `check` could warn on fragments that match no heading in the target
  (requires parsing the target's headings — cheap, already read).
- Explicit `{#anchor}` must keep winning; slugs only fill the gap.
