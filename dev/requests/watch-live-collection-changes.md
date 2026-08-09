# Request: `watch` should notice files created or deleted during the session

Affects the `fix/notebook-navigation` branch (PR #39) and earlier.

## Symptom

The interactive server builds its notebook set once at startup
(`build_state`), and the watcher's path→slug map (`by_path` in
`render_loop::spawn`) is a startup snapshot. Consequences, all silent:

- A `.md` created while the server runs never appears in the listing, is
  never served (`/n/<slug>` → 404), and its change events are dropped
  with **no log line** — despite the comment on the watch-target setup
  claiming dir mode "catches new files".
- Renaming `_draft.md` → `draft.md` (exactly what the partial convention
  invites) needs a restart.
- Deleting a notebook leaves a stale listing entry serving the last
  rendered HTML.
- An `index.md` created after startup is never picked up
  (`index_md_path` is `None` for the session).
- Body links to a not-yet-known file stay unresolved until restart
  (`link_slugs` is startup-frozen).

## Proposal

Handle create/remove/rename events in the coordinator: rebuild the
listing (slugs for new files via `unique_slug` over the existing set, so
established URLs never change mid-session), refresh `ServerState`
(requires interior mutability for `notebooks`/`order`/`link_slugs` —
today they're plain fields behind an `Arc`), re-render neighbours whose
prev/next changed, and push an index-refresh. Log every add/remove.

Note the slug-stability caveat documented at `unique_slug`: positional
`-N` suffixes mean a mid-session add can't renumber existing collision
groups without breaking baked hrefs — new files must take the next free
suffix instead.

## Related session-frozen state (same root cause)

`Notebook::title` is read once: renaming an H1 updates the page's own
crumb but every sibling's prev/next label and the index listing stay
stale for the session. The same rebuild path should refresh titles.
