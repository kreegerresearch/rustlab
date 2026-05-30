# Persistent Function-Result Cache (project-level memoization)

**Current phase:** Phase 6 — Optional polish
**Status:** Phases 0–5 complete; Phase 6 partial (6a inline-fn purity
gate + 6b per-function stats shipped; 6c transitive purity walker
landed but unwired; `#[memoize]` proc-macro, `%%cache` cell
directive, zstd compression, cross-process counter aggregation, and
per-function-within-a-file hashing remain deferred).

## Motivation

Notebooks and scripts frequently re-run cells/files that perform expensive
computations (large FFTs, eigensolves, dataset processing) whose inputs
haven't actually changed. The existing block cache in
`crates/rustlab-notebook/src/cache.rs` is an in-memory, watcher-session
prefix cache that helps **within** a single `notebook watch` session, but:

- It evaporates when the watcher restarts.
- It can't help one-shot `notebook render` invocations or REPL/script use.
- It can't share results across notebooks/scripts that recompute the same
  thing.
- It invalidates an entire suffix when any earlier block changes.

This plan adds a **persistent, function-call-level cache**. After a single
`cache enable` line, every user-defined function whose body takes ≥100 ms
to run has its result stored in a SQLite DB keyed on (function-AST hash,
input fingerprint). Subsequent calls with identical inputs return the
stored value in ~ms. Orthogonal to and complementary with the existing
block cache.

## Locked design decisions

Captured from the design conversation on 2026-05-24.

### Activation model

User explicitly opts in once per session/notebook/script with a `cache`
statement. Every call to an in-scope function is looked up; every miss
that produces a serializable result is stored. **No time-based gating
on the store side** — the threshold-based "only store if slow"
machinery was removed by user direction on 2026-05-24. Cache size is
managed by the explicit `cache prune` / `cache clear` commands.

```
cache enable                              % opens/creates .rustlab/cache.db
cache enable "my_cache.rcache"            % user-named store (portable)

cache add file helpers.rlab               % source the file, memoize its fns
cache add function expensive              % memoize one already-defined fn
cache add function f1, f2                 % multiple

cache remove function expensive
cache off

cache status
cache clear
cache prune older=30d
```

Sugar: `cache "my.rcache"` is an alias for `cache enable "my.rcache"`.

Default scope after `cache enable` is `all` — every user-defined function
called from here on is eligible. `cache add` narrows or extends; `cache
remove` drops from scope (does not delete DB entries).

### Storage

Two store kinds, identical schema:

1. **Per-project default** (`cache enable` with no path) → `.rustlab/cache.db`
   in the workspace root. `.gitignore` excludes `.rustlab/` workspace-wide.
2. **User-named** (`cache enable "name.rcache"`) → user-chosen path. User
   decides whether to commit it (ship a pre-warmed notebook) or ignore it.

Path resolution: relative paths resolve to the calling artifact's
directory (notebook dir / script dir) or CWD (REPL). Path is canonicalized
on open so different spellings of the same file collide correctly.

At most one store is active per process. `cache enable` while another is
active closes the prior store first.

### Caching policy

- **Lookup path** always runs for in-scope functions: fingerprint
  inputs, single SQLite point-query.
- **Store path** always runs on a miss when the result can be
  serialised. Every cacheable call grows the store by one row. The
  original threshold-based gate was removed on 2026-05-24 to keep the
  mental model "if it's in scope, it's cached" rather than "if it's
  in scope AND slow enough."
- Trade-off: a function called many times with varied args fills the
  DB faster than the threshold design predicted. Users manage size
  with `cache prune older=...` / `cache clear` / `rm .rustlab/cache.db`.
  The Phase 6 deferred items include "automatic LRU/TTL daemon" if
  this proves painful.

### Function identity

- **Inline-defined functions** (REPL / notebook code block): identity =
  BLAKE3 of the parsed function body. Whitespace/comment-stable; bumps
  automatically when semantics change.
- **Functions loaded via `cache add file helpers.rlab`**: identity =
  `BLAKE3(file_ast_hash || fn_name)`. Editing any function in the file
  busts entries for **every** function from that file. Coarser than
  per-function file-relative hashing, but it sidesteps the
  transitive-callee-hash problem; if function `B` in the file calls `A`,
  both rehash automatically when either is touched. May split per-function
  later if cache thrash becomes a real complaint.

Identifier renames intentionally bust the key — name capture into a free
variable could otherwise corrupt the cache silently. Renames = "different
function" by definition.

### Purity contract

Functions reach the cache only if they are pure with respect to inputs
and outputs:

- **Free variables**: any reference to a name not bound as a parameter
  or local is a hard error at `cache add file …` time:
  `"helpers.rlab:42 references unbound symbol 'k' — pure functions only"`.
- **Impure builtins**: `rand`, `randn`, `randi`, `now`, `tic`/`toc`, any
  file I/O without a path fingerprint, any plot call. Detected during the
  function's AST walk against a denylist maintained next to the script
  builtin registry. Policy:
  - In `all` mode (the default) — silently skip caching for that function.
    `cache status` shows which functions were skipped and why.
  - In explicit mode (`cache add function expensive`) — hard error. The
    user asked; the user gets told.
- **Non-serializable result types**: function handles, file handles, any
  `Value` variant we can't round-trip. Same skip-vs-error split.

### Large-input fingerprinting

Lazily-computed `OnceCell<[u8; 32]>` fingerprint slot on `Matrix`,
`CMatrix`, `SparseMatrix`, cell-array, and struct `Value`s. Invalidated
on every mutation path. Files are fingerprinted as
`(canonical_path, mtime_nanos, size_bytes)`.

NaN inputs short-circuit to "uncacheable, just run it" with a `--verbose`
warning; canonical byte form has no representation for NaN equality.

### Eviction

Manual. The DB stores only the minimum needed: function-id hash, input
hash, result blob, blob size, created-at. `cache prune` and `cache clear`
commands plus "just `rm .rustlab/cache.db` (or your `.rcache`) and it
recomputes" as the failsafe. No automatic LRU/TTL daemon.

### Multi-instance behaviour

Two `notebook watch` processes, REPL + render, CI + developer — all
realistic. The design accepts concurrency at the SQLite layer:

- `PRAGMA journal_mode=WAL` from day one. Many readers + one writer,
  crash-safe (a killed process leaves a WAL the next opener replays).
- `INSERT OR IGNORE` on `put` so simultaneous cold-misses on the same key
  don't error out. Both processes' computes are wasted on the loser's
  side; document this as "results stay correct, only CPU is wasted."
- **Per-process** active-store / scope / counters. `cache status` reports
  only this process. `cache list` reads the DB directly and reflects
  cross-process state because it's read-only.
- Schema-version mismatch: newer-binary writes a newer
  `schema_meta('writer_min_version', …)`. Older binary opening a newer
  schema **silently treats it as cold** (no writes from the old binary,
  no user-facing warning). Newer binary opening older schema migrates in
  place if a migration exists; otherwise treats as cold.
- Read-only filesystem on the chosen path → clear error at `cache enable`:
  `"cannot open '.rustlab/cache.db': read-only filesystem"`.
- Disk full mid-write → log `"cache write failed: disk full"` and continue
  without storing. Caching is an optimization, never load-bearing.
- **NFS detection deliberately skipped.** SQLite locking on NFS is known
  to be flaky; document the caveat once in `docs/notebooks.md` and let
  the user pick a local `.rcache` path if they hit problems.

## Non-goals

- Caching for Rust functions in the rustlab codebase itself (could be a
  follow-up; not needed for the notebook/script/REPL use case).
- Distributed/networked cache. Local file only.
- Cross-machine cache sharing. Path canonicalization is per-host; shipping
  a `.rcache` between machines gives warm entries only for the AST+input
  combos that line up.
- Caching of side-effectful operations. Enforced by the purity contract.
- Automatic eviction. Manual `cache prune` / `cache clear` only.

## Crate layout

New crate **`crates/rustlab-cache`** — runtime layer:

- SQLite open / schema / migrations / WAL config
- `Fingerprint` trait (impls live in `rustlab-core` for `Value` variants)
- `get(entry_id, input_hash) -> Option<Bytes>`
- `put(entry_id, input_hash, bytes)` with `INSERT OR IGNORE`
- Active-store registry (per-process)
- Eviction / prune / clear helpers

Hangs off `rustlab-script` (not `rustlab-notebook`) so REPL / script /
notebook entry points all get it. The `rustlab` main binary therefore
grows by the rusqlite-bundled cost (~1.2 MB release-stripped) — **this is
a conscious override of the "keep the rustlab binary small" rule**
(`feedback_rustlab_binary_size`), because making cache REPL-accessible
was the user's stated design goal.

Future-proofs for a Rust-level `#[memoize]` proc-macro crate
(`rustlab-cache-macros`) without re-plumbing the runtime.

## Trade-off study required (per AGENTS.md)

Before Phase 1 lands, file `dev/plans/persistent-function-cache-tradeoff.md` covering:

- `rusqlite` (bundled-sqlite) vs hand-rolled append-only log + index
- BLAKE3 vs alternatives for input hashing (already used by the notebook
  JSON renderer's source hash — short note suffices)
- `bincode` vs `postcard` vs custom serializer for `Value` blobs

This crate is **infrastructure** (storage + hashing), not core numerics,
so external libraries are acceptable per `feedback_licensing`. The
trade-off doc justifies the specific choices.

## SQLite schema (Phase 1 draft)

```sql
CREATE TABLE IF NOT EXISTS cache_entries (
  entry_id        BLOB NOT NULL,   -- BLAKE3 of (file_ast_hash || fn_name)
                                   -- or BLAKE3 of inline fn AST
  input_hash      BLOB NOT NULL,   -- BLAKE3 of canonical input fingerprint
  value           BLOB NOT NULL,   -- serialized Value
  bytes           INTEGER NOT NULL,-- len(value) for prune
  rustlab_version TEXT NOT NULL,   -- env!("CARGO_PKG_VERSION") when written
  created_at      INTEGER NOT NULL,-- unix seconds
  PRIMARY KEY (entry_id, input_hash)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS schema_meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
-- schema_meta('version','1')
-- schema_meta('rustlab_version', env!("CARGO_PKG_VERSION"))
-- updated on every open; reflects the latest rustlab that touched this DB
```

`WITHOUT ROWID` keeps the table compact for the (BLOB, BLOB) primary key.
No indexes beyond the PK — prune scans the whole table, fine at expected
sizes (<100k rows).

## Phases

### Phase 0 — Design & dependency trade-off  **Status:** complete

- [x] Wrote `dev/plans/persistent-function-cache-tradeoff.md` (rusqlite, BLAKE3 reuse, rmp-serde)
- [x] Pinned the `Fingerprint` trait signature and canonical byte form
      per `Value` variant (sorted struct field order, fixed
      little-endian, host-independent)
- [x] Locked the `Value` round-trip support set: `Scalar`, `Complex`,
      `Bool`, `Str`, `Vector`, `Matrix`, `Tuple`, `Struct`,
      `StringArray`, `SparseVector`, `SparseMatrix`, `FuncHandle`,
      `None`. Other variants explicitly refuse to round-trip
- [x] Locked the impure-builtin denylist (`purity.rs::IMPURE_BUILTINS`
      with a `binary_search` sort-invariant test). Names are listed
      whether or not the corresponding builtin is currently registered
      — future-proofing means `tic`/`now`/`fopen`/etc. can't be
      accidentally cached if/when they ship
- [x] Per-file-AST hash (coarse) is the v1 strategy; per-function-
      within-file-context deferred to Phase 6 if cache thrash is reported

### Phase 1 — Storage layer  **Status:** complete

- [x] Added `crates/rustlab-cache` workspace member
- [x] Workspace deps: `rusqlite = { version = "0.31", features = ["bundled"] }`,
      `blake3 = "1"`, `tempfile = "3"` (dev)
- [x] `Store::open` lazily creates parents + opens with `PRAGMA
      journal_mode=WAL`, `PRAGMA synchronous=NORMAL`
- [x] Schema version 1; every open updates
      `schema_meta('rustlab_version', env!("CARGO_PKG_VERSION"))`;
      each row also records its own `rustlab_version` column for
      version-aware prune
- [x] `get`/`put` with `INSERT OR IGNORE`; transient-write errors
      (`SQLITE_FULL`, `SQLITE_READONLY`, `SQLITE_CANTOPEN`) logged and
      swallowed
- [x] Unit tests: 16 covering round-trip, reopen, schema meta,
      OR-IGNORE, read-only directory, future-schema disables silently
- [x] **Multi-process stress test** in `tests/multi_process.rs`:
      spawns 4 child processes via `current_exe()`, runs for 2s with
      10 distinct keys, asserts ≤10 final rows and total puts may
      exceed 10 (duplicate-compute losers expected)
- [x] Schema-version-mismatch path: store opens in disabled mode,
      reads return `None`, writes are no-ops, no error

### Phase 2 — Hashing & fingerprint layer  **Status:** complete

- [x] `Fingerprint` trait in `rustlab-core::fingerprint`, returning
      `Option<[u8; 32]>` (BLAKE3) via a `feed`/`fingerprint` pair so
      composite types stream into a shared hasher
- [x] Implemented for `f64`, `i64`, `u64`, `bool`, `str`/`String`,
      `[u8]`, `Option<T>`, `[T]`, `Vec<T>`, tuples up to 3, `&T`,
      `C64`, `RVector`/`CVector`, `RMatrix`/`CMatrix`, `SparseVec`,
      `SparseMat`. Canonical byte form: sorted struct fields,
      little-endian, domain-separator tag per type, length-prefixed
      variable data, NaN propagates `false`
- [x] NaN handling: scalar and matrix NaN return `None`. Tests:
      `nan_makes_value_uncacheable`, `nan_in_matrix_propagates_to_none`
- [x] **Lazy fingerprint slot deferred.** `RMatrix`/`CMatrix` are
      `Array2<T>` aliases with no field-add room; v1 ships
      always-recompute. Phase 6 owns the decision between newtype
      wrapper / thread-local side-table / leaving as-is
- [x] `ast_hash::hash_function_body` for inline functions; `hash_stmts`
      for file AST; `function_entry_id(file_hash, fn_name)` for the
      file-loaded identity. All explicitly skip `Stmt.line` so
      moving a function by lines doesn't bust the cache
- [x] `purity::check_free_vars(body, params, is_builtin, is_sibling_fn)`
      and `purity::check_impurity(body)` walkers. Lambda params don't
      leak; FuncHandle to an impure name counts as a use
- [x] `cache_value::feed_value`, `fingerprint_args`, `serialize_value`,
      `deserialize_value` for the cacheable subset of `Value` (custom
      tagged binary, `WIRE_VERSION = 1`)
- [x] `rustlab_cache::file_fingerprint(path)` over
      `(canonical_path, mtime_nanos, size_bytes)` for path-input builtins

### Phase 3 — `cache` statement + evaluator wiring  **Status:** complete

- [x] Parser (Phase 3a): `cache enable [path]`, `cache off`,
      `cache add file <path>`, `cache add function <name>[, …]`,
      `cache remove function <name>`, `cache status`, `cache clear`,
      `cache prune [older=DUR] [max_size=BYTES]`. Sugar:
      `cache <path>` ≡ `cache enable <path>`. `Token::Cache` keyword,
      `StmtKind::Cache(CacheStmt)` AST node, 31 grammar tests
- [x] `CacheRegistry` (Phase 3b) holding active-store handle (`Arc<Store>`),
      scope sets (`all_scope`, `explicit_fns`, `removed_fns`,
      `file_loaded_fns`), and per-process `CacheCounters` (hits,
      misses, impurity_skips, free_var_skips, serialization_skips,
      uncacheable_arg_skips, plus per-function map from Phase 6b)
- [x] Evaluator wiring (Phase 3c) for each `CacheStmt` variant.
      Duration parser shared with the CLI via `rustlab_cache::parse_duration_secs`
- [x] Call dispatcher hook (Phase 3d) in `eval_user_fn_nargout`. Args
      that fail to fingerprint (NaN, non-cacheable variant) bypass on
      a per-call basis and bump `uncacheable_arg_skips`. Stored blobs
      that fail to deserialize bump `serialization_skips` and trigger
      recompute. Multi-output added in Phase 6d (see below)
- [x] `cache add file`: canonicalises path, parses, hashes the file
      AST, walks free-vars (hard error) and impurity (silent skip in
      file-load mode); installs every function in `user_fns` and
      registers the cacheable ones with `file_ast_hash`
- [x] `cache add function`: explicit-mode purity check is a hard error;
      otherwise registers the name in `explicit_fns`
- [x] Recursive `cache add file foo.rlab` from inside foo.rlab —
      currently not detected (deferred). Documented limitation
- [x] **Multi-output caching deferred to Phase 6d** (now shipped —
      see below)
- [x] Integration tests: `tests/cache_runtime.rs` (21 tests covering
      enable/off/add/remove/clear/prune/status) +
      `tests/cache_dispatch.rs` (13 tests covering real hit/miss
      behaviour, persistence across evaluator restarts, NaN bypass,
      function-body edits busting entry IDs)

### Phase 4 — CLI commands  **Status:** complete

- [x] `rustlab cache list [--limit N]` — short-hex keys, sizes,
      versions, timestamps. Never prints cached values
- [x] `rustlab cache prune [--older-than DUR] [--max-size BYTES]` —
      both optional. With neither, defaults to `--older-than 30d`
- [x] `rustlab cache clear` — wipes rows, keeps DB file
- [x] `rustlab cache status` — store path, schema version, rustlab
      version, entry count, total bytes, "disabled" flag on a
      future-schema DB
- [x] `--store PATH` flag on all four; defaults to `.rustlab/cache.db`
      CWD-relative
- [x] All four mirrored on `rustlab-notebook cache ...` (the notebook
      now depends on `rustlab-cache` + `anyhow`)
- [x] 8 integration tests in `crates/rustlab-cli/tests/cache.rs`
      driving the full binary via `Command`; populates the store with
      a `rustlab run` then round-trips status / list / clear / prune
      / missing-store / unknown-unit error

### Phase 5 — Docs, AGENTS, REPL help, examples  **Status:** complete

- [x] `docs/functions.md`: "Persistent Function Cache" section with
      grammar table, scope rules, purity contract, multi-instance
      caveats, worked example, CLI sub-section
- [x] `docs/quickref.md`: one-line `cache enable [path]` entry under
      Language with the full grammar inline
- [x] `docs/notebooks.md`: "Persistent function cache" section
      covering storage location, purity contract, CLI, multi-instance
      + NFS caveats, pointer to plan + demo
- [x] REPL `HelpEntry` for `cache` (full sub-form list + purity
      contract + CLI cheatsheet); `language → "Persistent cache"`
      category row; help-coverage tests pass
- [x] AGENTS.md "Active Plans" row points at this file with a
      condensed status line
- [x] `examples/notebooks/persistent_cache_demo.md` walkthrough +
      `examples/notebooks/cache_demo_helpers.rlab` helpers file used
      by the `cache add file` cell
- [x] Workspace `.gitignore`: `.rustlab/`

### Phase 6 — Optional polish  **Status:** partial

Shipped:

- [x] **6a** — Inline-fn purity gate at definition time. Closes the
      "inline impure fn under `all_scope` silently caches" door.
      `Evaluator::cache_gate_user_fn` runs `check_free_vars` +
      `check_impurity` on every `FunctionDef` while the cache is
      active, and `cache_enable` scans every pre-existing user
      function. Impurity → `remove_function` + `impurity_skips++`;
      free-var → `remove_function` + `free_var_skips++`
- [x] **6b** — Per-function hit/miss stats. `CacheCounters::per_fn:
      BTreeMap<String, FnCounters>` populated by
      `CacheRegistry::record_hit` / `record_miss`; surfaced as a
      sorted per-function table in `cache status`
- [x] **6c** — Transitive impurity walker. `check_transitive_impurity`
      with cycle detection wired into the gate at `FunctionDef` time;
      mutual recursion handled by re-scanning previously-removed
      siblings on each new `FunctionDef`. Catches `f → g → rand`
- [x] **7** — Canonical (rename-invariant, transitively-correct)
      function-identity hash. `ast_hash::canonical_entry_id` walks
      the AST through a scope stack that substitutes positional ids
      for parameters (`p0`, `p1`), return vars (`r0`, `r1`), and
      locals (`lN` in first-occurrence order). Sibling user-fn calls
      are folded in by recursively computing the callee's canonical
      hash (cycle-broken with name fallback). Self-recursion uses a
      `self` marker. Builtin names stay verbatim. `Evaluator`
      memoizes the per-fn canonical id and invalidates en bloc on
      every `FunctionDef`. The file_ast_hash mixing for `cache add
      file` is dropped — both inline and file-loaded functions share
      the algorithmic identity, so moving a function between files
      preserves cache warmth. Closes the correctness bug where
      editing an inline-defined callee would silently return stale
      cached caller results. Adds 10 dispatch tests covering
      param/local/return-var/fn-name renames, sibling rename without
      body change, callee-body-edit transitive bust, literal/operator
      changes still bust, self-recursion rename invariance, mutual
      recursion termination, lambda capture rename, and file→inline
      move.
- [x] **6d** — Multi-output caching. Wire-format bumped to v2;
      every user-function result stored as a `Value::Tuple` of the
      full canonical output set (one slot per declared return var,
      `Value::None` for unassigned). Cache key is nargout-
      independent — `p = stats(x)` and `[p, q] = stats(x)` share one
      entry, body runs once. `shape_user_fn_return` helper used by
      both the body-execution and cache-hit paths so the
      under-assignment / over-asking error semantics are byte-
      identical. `nargout = 0` calls populate the cache for free
      (warm-up). Old v1 blobs become silent `serialization_skips`

Deferred (none required to ship):

- Rust-side `#[memoize]` proc-macro for caching numeric helpers in
  the rustlab codebase itself (separate crate `rustlab-cache-macros`)
- `%%cache` cell directive in notebooks (sugar over `cache enable`)
- `zstd` compression for blobs above some size threshold
- Per-function-within-a-file hashing (instead of file-coarse) if
  cache thrash on edits becomes a real complaint
- Cross-process counter aggregation (would require a counters table)
- Transitive purity analysis (catches `f → g → rand`). The walker
  itself landed as `purity::check_transitive_impurity` but isn't
  wired into the gate yet — the order-of-definition caveat (a
  helper defined after its caller isn't retroactively detected) is
  documented; the gate would call it at FunctionDef + cache_enable
  time, matching the 6a pattern. ~30 lines to finish

## Resolved questions

1. **Threshold mechanism** — *removed entirely* on 2026-05-24 by user
   direction. The original design was a `threshold=N`/`elapsed >= N`
   gate on the store side; the simpler "every in-scope call writes"
   model shipped instead. `cache prune older=...` / `max_size=...` /
   `cache clear` are the size-management knobs.
2. **Notebook-cell-defined functions edited mid-watch.** Inline-fn
   AST hash changes → cache miss → recompute. The prior entry lingers
   until pruned. Accepted; documented in `docs/notebooks.md`.
3. **`cache add file` re-issued after edit.** Re-parse, re-hash,
   replace in-scope bindings, point registry at new entry_ids. Old
   rows linger until pruned. Documented.
4. **Cross-store sharing.** Out of scope. Two notebooks pointing at
   the same store path share entries; named-`.rcache` users opt into
   sharing explicitly by committing the file.

## Risks

- **AST instability across rustlab versions.** Bumping the parser may
  change AST node representation enough that entry_id hashes shift
  wholesale. Mitigation: `schema_meta('rustlab_version', …)` recorded
  per write; readers ignore future-schema versions (silent treat-as-cold).
- **Silent staleness from accidentally-impure functions.** A function
  passes the denylist check but reads a global through some indirection
  the walker missed. Mitigations: (a) the free-var check refuses to load
  files referencing unbound names, closing the most common door;
  (b) `cache clear` or `rm .rustlab/cache.db` as the documented "I don't
  trust the cache" escape hatch.
- **Concurrency races.** Accepted: simultaneous cold-misses produce
  duplicate compute. The Phase 1 stress test prevents harder regressions
  (corruption, lock errors, lost writes).
- **Binary-size regression on `rustlab`.** ~1.2 MB. Documented as a
  conscious override of `feedback_rustlab_binary_size`; revisit if the
  binary size becomes a user complaint.
