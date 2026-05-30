# Advanced Cache Behaviors and Limitations

A companion to `persistent_cache_demo.md`. The first notebook covers
the happy path; this one demonstrates the parts of the cache people
ask about: rename invariance, multi-output sharing, the `loaded`
status column, and the rough edges (impurity rejection, NaN bypass,
mutual recursion's name-fallback, stale-blob persistence).

## Setup

```rustlab
cache enable
cache clear
cache status
```

## 1. Rename invariance (Phase 7)

The cache key is **algorithmic**, not name-based. Renaming the
function, its parameters, locals, or return vars keeps the cached
result valid.

```rustlab
function y = expensive(x)
  k = 2
  y = x * x + k
end
a = expensive(20)
cache status
```

Now redefine with the same algorithm but every identifier renamed:

```rustlab
function w = compute(z)
  m = 2
  w = z * z + m
end
b = compute(20)
cache status
```

`compute` shows `1 hits, 0 misses` — the cached row written under the
original name is reused. The values are identical: both equal `402`.

## 2. Inspecting cached entries with `cache list`

```rustlab
cache list
```

The `status` column tells you:

- **`loaded`** — entry's `entry_id` matches a currently-defined function
- **`loaded (variant)`** — the function name matches a defined function but the cached body was a different version
- **`not loaded`** — no function with this name is currently defined in this session
- **`<unknown>`** — pre-metadata row from an older binary (the legacy compatibility path)

## 3. Multi-output caching (Phase 6d)

The cache key is `nargout`-independent. A `[a, b] = stats(x)` call
shares its cached row with `p = stats(x)` and `stats(x);`.

```rustlab
function [s, q] = stats(x)
  s = x + 1
  q = x * x
end
p = stats(7)
[u, v] = stats(7)
cache status
```

`stats: 1 hits, 1 misses` — the first call populated, the second hit
the same entry and reshaped the canonical tuple `[8, 49]` for the
multi-output destructure.

## 4. Limitation — impurity rejection (Phase 6a)

Functions that call non-deterministic builtins are **silently
excluded** from caching. Listed in the impurity counter so you can
spot them:

```rustlab
function y = noisy(x)
  y = x + rand()
end
a = noisy(1)
b = noisy(1)
cache status
```

Look at the `impurity_skips` counter: it bumped when `noisy` was
defined and the gate ran. `noisy` doesn't appear in the per-function
table because the dispatcher never routes through the cache for it.
The two calls produce different values because `rand()` is consulted
each time, which is the whole point.

## 5. Limitation — NaN bypass

`NaN` arguments can't fingerprint deterministically (IEEE: NaN ≠ NaN),
so the cache bypasses on a per-call basis:

```rustlab
function y = id(x)
  y = x
end
a = id(NaN)
b = id(NaN)
cache status
```

`uncacheable_arg_skips` shows the bypass count. The function still
runs normally — only the cache consultation is skipped.

## 6. Limitation — non-cacheable result types

If a function returns a value type the cache can't serialise
(e.g. an anonymous lambda, a live figure handle, a stateful FIR
buffer), the PUT step silently skips. The call works; the cache row
just stays absent.

```rustlab
function f = make_adder(k)
  f = @(x) x + k
end
g = make_adder(10);
cache status
```

`make_adder` produces a `Value::Lambda` — non-cacheable. You'll see
no cached row for it; the function name doesn't appear in the
per-function table.

## 7. Limitation — mutual recursion's name fallback

When two (or more) functions form a recursive cycle, the canonical
hash breaks the cycle by **falling back to the callee's name**
instead of recursing into its body. The result: cycle participants
retain rename-bust behaviour for the names involved in the cycle.
Everything else stays rename-invariant.

In practice: if you rename both halves of a mutual-recursion pair
(`ping`/`pong` → `alpha`/`beta`), the cache busts. Renaming a function
that isn't in a cycle keeps the cache warm.

## 8. Limitation — stale blobs after a rustlab upgrade

When the cache's wire format changes between rustlab versions, the
new binary can't deserialise the old rows. The dispatcher detects
the mismatch, bumps `serialization_skips`, and falls through to a
fresh compute — but the PUT step uses `INSERT OR IGNORE` (which is
correct for concurrent-writer race-handling) so the stale row stays
in place. On every subsequent call you eat another skip + recompute
until you clear the cache.

The escape hatch is documented: after upgrading rustlab and noticing
elevated `serialization_skips`, run `cache clear` (or `cache prune
older=...` for selective cleanup).

## 9. Cleanup

```rustlab
cache clear
cache status
```

Outside this notebook, the same operations are available via the CLI:

```sh
rustlab cache status
rustlab cache list --limit 20
rustlab cache prune --older-than 30d
rustlab cache clear
```

The four subcommands are mirrored on `rustlab-notebook cache ...`
for notebook-driven workflows.
