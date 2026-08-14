# Request: refresh the served index title (and H1) when index.md's title changes

Affects the `fix/notebook-navigation` branch (PR #39).

## Symptom

The watch server reads `index.md`'s title once at startup
(`ServerState.index_title` is a plain `String`). Editing the body during
a session refreshes the served `/` page (`index_body` is behind an
`RwLock`), but a title edit (`# Home V1` → `# Home V2`) leaves
`<title>` and the `<h1>` at the old value until restart. Documented at
`refresh_index`, deliberate for the initial implementation.

## Proposal

Move `index_title` behind the same `RwLock` (or fold both into one
`RwLock<(String, String)>`), and have `refresh_index` publish the title
alongside the body. The listing itself derives from per-notebook titles,
which are a separate session-frozen issue — see
[watch-live-collection-changes](watch-live-collection-changes.md).
