# Trade-off study: dependencies for `rustlab-cache`

The persistent function-result cache (plan:
[persistent_function_cache.md](persistent_function_cache.md)) needs three
infrastructure libraries: an embedded key-value/SQL store, an input hash,
and a value serializer. This is infrastructure (storage + hashing), not
core numerics, so per `feedback_licensing` external libraries are
acceptable — but per AGENTS.md § 10 the choices need a written study
before any code lands.

## Trade-off study: `rusqlite` (bundled) for the key-value store

### What we'd hand-roll

A single-file append-only log (`.rustlab/cache.db`) of records:

```
[ entry_id (32) | input_hash (32) | bytes_len (u32 LE) | value (bytes_len) | xxh3 (8) ]
```

plus an in-memory `HashMap<(entry_id, input_hash), file_offset>` rebuilt
on open by scanning the log. Concurrent writers coordinate via a sidecar
lockfile + `flock(2)`; readers tolerate truncated tail records by
checksum verification.

- Rough cost: ~600 LoC for store + index + truncation recovery; ~250 LoC
  more for cross-process locking that doesn't silently corrupt under
  crash; ~5 days of senior work end-to-end, mostly tests.
- Risk surface:
  - Wrong fsync discipline → corruption on power loss.
  - Wrong lockfile semantics on macOS (`flock` over APFS) vs Linux
    (`flock` over ext4) → flaky multi-process behaviour.
  - Index rebuild cost grows with DB size; we'd add periodic compaction.
  - No transactional pruning — `cache prune` becomes a "rewrite the
    whole file" operation. Easy to corrupt under signal.
- In-tree location: `crates/rustlab-cache/src/store.rs` + `index.rs` +
  `lockfile.rs`.

### What `rusqlite` gives us

- Battle-tested embedded SQL store. We use a single table; the SQL
  surface we touch is `CREATE TABLE`, `SELECT`, `INSERT OR IGNORE`,
  `DELETE`, `PRAGMA`. ≈10 distinct statements total.
- WAL journal mode → multi-reader + single-writer concurrency with
  crash-safety. SQLite replays the WAL on the next open after a kill.
- `INSERT OR IGNORE` for our "duplicate cold-miss" path falls out for
  free.
- Pruning is a `DELETE WHERE created_at < ?` in a transaction — atomic,
  no rewrite-the-file dance.
- `bundled` feature compiles SQLite from source into the crate. No
  system-sqlite dependency, no version skew between dev machines,
  identical behaviour on CI and laptops.

Crate facts:

- License: MIT (rusqlite) + SQLite is public domain. Compatible with
  the workspace's MIT-or-Apache-2.0.
- Latest stable: `0.31` (Q1 2024-vintage; SQLite ~3.45). Active
  maintenance — last release roughly quarterly.
- Major-version churn: rusqlite has been ~one major bump per year.
  Migration cost has historically been small for the surface we use.
- Transitive deps: `libsqlite3-sys` (vendored C), `bitflags`, `fallible-iterator`,
  `fallible-streaming-iterator`, `hashlink`, `smallvec`. ~6 small crates.
- Compiled size: ~1.2 MB added to `rustlab` (release, stripped) — this
  is the binary-size hit called out in the plan. AGENTS.md § 10 flags
  >1 MB as "suspect for core work"; we are explicitly *not* core work,
  and the plan documents this as a conscious override of
  `feedback_rustlab_binary_size`.

### Pros of pulling it in

- Saves ~5 days of build-time-and-test-burden work that is *entirely*
  off rustlab's value proposition.
- The crash-safety / WAL / `INSERT OR IGNORE` properties we want are
  exactly the properties SQLite was built to provide. Hand-rolling
  these correctly is non-trivial.
- One library covers store, index, transactions, and pruning. The
  hand-roll splits into three coupled subsystems.
- `cache prune` and `cache list` queries are trivial in SQL; ad-hoc
  index code would need new code paths per query.

### Cons of pulling it in

- ~1.2 MB binary growth on the main `rustlab` binary.
- ~6 transitive crates + vendored C (libsqlite3-sys). SQLite the C code
  is the largest piece of "code we can't debug to the line" we'd pull
  in; mitigated by the fact that ~all bugs are someone else's already.
- C compilation in the build (libsqlite3-sys). Hurts cold-build time on
  CI by ~10 s and requires `cc` on the build host (already required
  transitively by other crates in the tree — no new system dep).
- Larger API surface than we strictly need.

### Recommendation

**Pull in `rusqlite = { version = "0.31", features = ["bundled"] }`.**

The crash-safety + WAL + transactional-prune properties are load-bearing
for the multi-instance design and are exactly what the plan accepts as a
conscious binary-size override. The hand-roll alternative is real work
on plumbing rustlab doesn't want to own.

Pin: `0.31.0`. Re-evaluate when rusqlite bumps to `0.32`.

## Trade-off study: BLAKE3 for hashing (input fingerprint + AST hash)

### Context

We already use `blake3` for the notebook JSON renderer's source hash
(`crates/rustlab-notebook/src/render/json.rs` — established precedent).
The cache layer needs the same primitive for input fingerprinting and
AST hashing.

### Alternatives considered

- **xxHash3** — faster than BLAKE3 on small inputs (sub-100 B), but not
  cryptographic-strength. Hash collisions in our cache key would
  silently return wrong cached results — different inputs, same key.
  Probability is astronomically low at our scale, but BLAKE3 closes the
  door entirely and the perf difference doesn't matter when we're
  hashing matrices.
- **SHA-256** — cryptographic, but slower than BLAKE3 on the matrix-sized
  inputs we actually care about, and not already in the tree.
- **`std::hash::DefaultHasher`** — explicitly documented as unstable
  across Rust versions and not portable across hosts. Disqualified for
  any persisted hash.

### Recommendation

**Reuse `blake3 = "1"` (already a workspace dep candidate; one new
workspace deps entry needed).** No trade-off study tension: the choice
falls out from "we use it already" + "it's the right primitive for the
job."

## Trade-off study: `Value` blob serializer

### Context

Cache stores serialized `Value` blobs. Phase 1's `get`/`put` API operates
on opaque `Vec<u8>`; the serializer choice doesn't affect the storage
layer at all — it's a Phase 2/3 concern. We pre-decide here so the
storage tests can use realistic blob sizes.

`Value` (in `crates/rustlab-script/src/eval/value.rs`) currently does
**not** derive `Serialize`/`Deserialize`. Adding those derives is the
Phase 2 prerequisite, regardless of which framing format we pick.

### Alternatives

- **`rmp-serde`** (MessagePack) — already a workspace dependency
  (`rustlab-proto`, `rustlab-viewer`, `rustlab-plot` with `viewer`
  feature, `rustlab-script` test code). Self-describing. ~1.5× slower
  than postcard/bincode but the gap is irrelevant when the dominant
  cost is the function we're caching (≥100 ms by definition).
- **`postcard`** — `no_std`-friendly, smallest output, fast. Not in
  tree. Strict schema discipline required (postcard is *not*
  self-describing) — any `Value` variant reorder breaks all cached
  blobs. We'd need our own versioning shim.
- **`bincode 2`** — recently bumped major version; mixed migration
  experience reported across the ecosystem. Not in tree.
- **Custom binary format** — overkill for an internal cache.

### Recommendation

**Use `rmp-serde` (workspace dep, already pinned to `"1"`).**

Zero new dependencies. Slight binary-size cost is paid already. Slight
runtime overhead is below the noise floor against a ≥100 ms compute
threshold. Self-describing framing means reordering `Value` variants
between rustlab versions is tolerated — old blobs deserialize against
the new variant set as long as the wire-level field names line up. (For
hard-incompatible schema changes we lean on the per-row
`rustlab_version` column + `cache prune` rather than serializer
versioning.)

## Summary

| Library     | Status                | Decision                                  |
|-------------|-----------------------|-------------------------------------------|
| `rusqlite`  | New (bundled C)       | **Pull in**, pin `0.31`                   |
| `blake3`    | Reuse existing precedent | **Reuse**, workspace dep at `"1"`      |
| `rmp-serde` | Already workspace dep | **Reuse** (Phase 2/3 wiring; no Phase 1 impact) |

Hard limits per AGENTS.md § 10:

- ✅ All MIT / public domain — no copyleft.
- ✅ Pure Rust + vendored C (SQLite). No Fortran / C++ FFI.
- ⚠️  Binary size: +1.2 MB from rusqlite-bundled. Explicit override per
   plan; user signed off in design conversation.
- ✅ Not vendoring curriculum-relevant numerics. SQLite is plumbing.
