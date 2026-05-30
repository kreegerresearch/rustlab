# Persistent Function Cache

This notebook walks through the `cache` statement: open a store at the
top of the notebook, define a deliberately-slow function, and watch
the second call return without recomputing. The cache survives
`notebook render` restarts and is shared across processes that point
at the same store path, so warmth carries between watch-mode reloads
and even between CI runs that ship a committed `.rcache`.

## What this saves you

The interesting number is the **hit-path latency floor**: ~1 ms,
regardless of how long the original computation took. A function
whose first call takes 3 seconds returns from the cache on the
second call effectively instantly. The
"second call" can be next week, next CI run, or another notebook
that points at the same store. If you re-render this notebook
locally with `notebook watch`, the first render does the work; every
subsequent render is dominated by parsing and prose pipeline, not by
`heavy()` below.

## Enable the cache

`cache enable` with no path opens (or creates) the per-project store
at `.rustlab/cache.db`. The workspace `.gitignore` excludes
`.rustlab/` by default — see `docs/notebooks.md` § "Persistent
function cache" for the named-store form when you want to commit a
warm cache.

```rustlab
cache enable
cache status
```

`cache status` should now report **active**, the store path, and zero
counters across the board.

## A deliberately-slow function

`heavy(n)` runs a few thousand iterations of a synthetic loop so it
takes long enough that the speedup is observable. The function is
pure: parameters in, scalar out, no globals, no plotting, no RNG.
That's the purity contract the cache enforces — the new inline-fn
gate (Phase 6a) refuses to cache impure functions even under the
default `all` scope.

```rustlab
function y = heavy(n)
  y = 0
  for i = 1:n
    y = y + sqrt(i) * sin(i / 100)
  end
end
```

## First call — miss

The first call computes the answer and writes the result to the
store.

```rustlab
a = heavy(50000);
disp("first call complete")
cache status
```

`cache status` now shows `1 misses` and one row under `per function`
for `heavy`. The store on disk has one entry — the
`(heavy_ast_hash, fingerprint_of_50000)` pair.

## Second call — hit

Same arguments → cache hit. The function body never runs; the
dispatcher returns the stored `Value` directly.

```rustlab
b = heavy(50000);
disp("second call complete")
cache status
```

Notice `1 hits, 1 misses` and `heavy` showing `1 hits, 1 misses` in
the per-function table. `a` and `b` are bit-identical.

**What this saved**: the work `heavy(50000)` did the first time —
50,000 iterations of `sqrt(i) * sin(i / 100)` summed in
single-threaded interpreted rustlab — didn't happen on the second
call. The dispatcher fingerprinted the argument `50000`, looked up
the matching row in `.rustlab/cache.db`, deserialised the stored
`Value::Tuple([Scalar(1.886e6)])`, and returned. From your
perspective: the result was already there.

## A different argument is a different key

Distinct inputs produce distinct keys. The next call misses (it's a
new key); a third call with the same `60000` would hit.

```rustlab
c = heavy(60000);
cache status
```

## Loading a file of helpers in one shot

`cache add file <path>` sources a `.rlab` file and registers each
top-level function as cacheable. Free variables are a hard error;
impure helpers in the file are silently skipped (still installed, just
not cache-routed). Sibling-function calls within the file are
recognised.

We ship a small helpers file next to this notebook —
`examples/notebooks/cache_demo_helpers.rlab` — with two functions:
`square(x) = x .* x` and `sum_of_squares(n) = sum(square(1:n))`.

```rustlab
cache add file "cache_demo_helpers.rlab"
s = sum_of_squares(2000);
fprintf("sum_of_squares(2000) = %.0f\n", s)
cache status
```

## Cleaning up

Wipe the store when you don't trust it (e.g. after a rustlab upgrade,
or while debugging a function whose behaviour you've changed
externally). The DB file is kept; only the rows go.

```rustlab
cache clear
cache status
```

Outside a notebook, the same operations are available via the CLI:

```sh
rustlab cache status
rustlab cache list --limit 20
rustlab cache prune --older-than 30d
rustlab cache clear
```

The four subcommands are mirrored on `rustlab-notebook cache ...` for
notebook-driven workflows.
