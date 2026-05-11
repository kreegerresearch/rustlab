# Implementation Plan — `parmap` / `parreduce` (rayon-backed parallel map / reduce)

> **For the next agent:** This is both the *reference* doc (rationale, decisions, file landmarks) and the *action* doc (status table, per-phase steps). Read top to bottom. The user is **still brainstorming**; this plan was written from the design conversation in session 2026-05-10 and **has not been approved for implementation yet**. Do not start coding without explicit "go" from the user.
> Source design conversation: turns near the end of session 2026-05-10. The user has a Monte Carlo workflow and asked whether rustlab can run lambdas in parallel. The chosen pattern is **explicit map-reduce** (`parmap(f, xs)` and optionally `parreduce(f, init, xs)`), not auto-parallelizing lambdas. See "Decisions already locked" below.

**Date opened:** 2026-05-10
**Date approved:** 2026-05-11 (plan-level scope + all 7 design questions locked).
**Plan status:** Approved. Phase 1 unblocked. Phase 1 may begin when the user signals "go" — approval ≠ implementation kickoff.

## Status snapshot

| # | Phase | Status | Risk | Win |
|---|---|---|---|---|
| 1 | `Evaluator: Clone + Send`, AST `Serialize + Deserialize` (Value-level serde rescoped to Phase 2) | **shipped** | medium-high | `20d4a82` |
| 2 | `parmap(f, xs)` builtin behind `ParmapBackend` trait (local rayon impl) | **shipped** | medium | `98d084c` |
| 3 | Per-task RNG + pure-lambda contract | **shipped** | low | `0d789c4` |
| 4 | Tests + docs + REPL help, including `nproc()` builtin | **shipped** | low | `0d789c4` |
| 5 | `parreduce(f, init, xs)` (follow-on) | **deferred** | low | only build if a concrete use case demands it |
| 6 | Cluster backend via `rustlab-server` (separate plan) | **deferred** | n/a | distributed compute; placeholder so Phase 2's trait shape stays honest |

*Status legend:* `pending` (not started) · `in progress` (branch open) · `awaiting commit` (code/tests/docs landed locally, not yet committed) · `blocked` (note why) · `shipped` (commit hash).

## Decisions already locked (do not re-litigate)

The user explored auto-parallelism vs explicit map-reduce in conversation. Conclusion:

1. **Map-reduce over auto-parallelism.** Auto-parallelizing lambdas in a dynamic language requires data-flow analysis the interpreter doesn't have. Heuristics silently break correctness on innocent-looking code. Explicit `parmap(f, xs)` puts the "this is parallelizable" decision in the user's hands.

2. **`parmap` is a builtin, not a syntax form.** No `parfor k = 1:N; …; end`. The map-reduce surface (`parmap`, optional `parreduce`) covers the same expressive ground without parser/evaluator changes for new statements.

3. **rayon is the parallelism primitive.** User confirmed (mid-em_performance Phase 3) that rayon is acceptable infrastructure — it provides parallel orchestration, not numerics. AGENTS.md Rule 9 still applies: rayon is fine for outer-loop parallelism over hand-rolled kernels; we don't import third-party math libraries.

4. **Lambdas must be pure inside `parmap`.** No plotting (`clf`, `figure`, `plot`, `imagesc`, `quiver`, `streamplot`), no file I/O (`fprintf`, `fopen`, `csvwrite`), no audio (`AudioOut` writes), no FIR streaming state mutation, no `LiveFigure` writes, no `clear`/`format`/`seed` calls. The runtime enforces this at the offending call site (better than silent wrong answers).

5. **Per-task RNG state.** Each parallel task gets its own RNG, deterministically derived from a master seed (set via `seed(N)` before the `parmap` call). Across runs, `seed(K); parmap(...)` produces bit-identical results.

6. **No subprocess fallback.** If `parmap` exists, it does in-process parallelism. The shell-level `xargs -P` / GNU parallel pattern still works for users who want process isolation; we don't try to subsume it.

7. **Design questions resolved (2026-05-11).** All seven open questions answered — see the "Design questions — answers locked" section for the per-item resolution. TL;DR: `Lambda` + `FuncHandle` accepted; 1-D iterables only in v1; scalar-return → vector, anything else errors; no `"threads"` knob in v1 (`nproc()` exposes the pool size); errors cancel + propagate; pure-lambda contract is a hard error; profiler disabled inside parmap.

## Why this design over alternatives

| Alternative | Why we're not doing it |
|---|---|
| Implicit `for k = 1:N` auto-parallelism | Requires data-flow analysis (proving iterations are independent) the interpreter doesn't have. Heuristics break correctness. |
| `parfor k = 1:N; …; end` syntax | Adds a new statement form to parser/evaluator for no expressive gain over `parmap`. |
| OpenMP-style pragmas | Special syntax, hard to teach; rustlab is interpreted so the compile-time-pragma metaphor doesn't fit. |
| async/await + futures | Heavy machinery for CPU-bound work. MC is not I/O-bound. |
| Subprocess pool only | Already works today via `xargs -P`; per-process startup is ~100 ms. Hurts when trials are fast. We want in-process for this case. |
| Auto-detect "pure" lambdas | Static purity analysis in a dynamic language is unreliable. Better to let the user opt in via `parmap` and enforce purity at runtime. |

## Future compatibility — distributed backend

The user has flagged a future `rustlab-server` for distributed compute. The `parmap` design here is deliberately set up so that backend can land additively, without changing any user-facing scripts. Two-backend architecture:

| Backend | What it is | When it picks |
|---|---|---|
| `local` (Phases 1–4 of this plan) | rayon thread pool on this machine | default; no server pool configured |
| `cluster` (deferred Phase 6) | dispatch to `rustlab-server` workers over the network | only when a pool is configured (env var or builtin) |

User-facing surface is identical between backends. `parmap(f, xs)` doesn't care whether workers are threads on this box or processes on other boxes.

### Forward-compat constraints baked into Phase 1 / Phase 2

These are cheap to add now and expensive to retrofit:

1. **`#[derive(Serialize, Deserialize)]` on the AST.** The lambda's body (`Expr`) needs to cross the wire intact in distributed mode. Adding the derives now costs ~150 LoC of `derive` annotations on `ast::Expr`, `Stmt`, and friends; doing it later means a sprawling cross-cutting refactor on a larger AST. Done as part of Phase 1's audit pass.

2. **`#[derive(Serialize, Deserialize)]` on the pure `Value` variants.** `Scalar`, `Complex`, `Vector`, `Matrix`, `Tensor3`, `Bool`, `Str`, `Sparse*`, `Tuple`, `Struct`, `StringArray`, `Lambda` (recursively). Stateful variants (`FirState`, `LiveFigure`, `AudioIn`/`Out`) deliberately do NOT serialize — they're already excluded by the pure-lambda contract from Phase 3. Their `Serialize` impl errors with the same contract message.

3. **`ParmapBackend` trait abstraction in Phase 2.** The local rayon implementation lives behind `trait ParmapBackend { fn run(&self, …) -> … }`. The `parmap` dispatch in `eval_expr` calls through the trait, not directly into rayon. The trait surface is small (one method); the future cluster impl is purely additive — zero changes to existing code paths.

4. **Treat the captured-env + input element as the serialization boundary even in the local backend.** `serialize → invoke worker → deserialize result`. For shared-memory rayon this is wasteful overhead (sub-microsecond per call via msgpack/`rmp-serde`) but it's a *forcing function*: it keeps the pure-lambda contract honest and makes "local-only bug, fails in cluster mode" impossible to ship. **Subject to confirmation during Phase 1** — if the cost is non-trivial on small per-call payloads, we drop this for the local backend and trust the contract instead.

5. **Generic names for any tunable knobs.** `"chunks"`, `"threads"` are fine; rayon-specific names like `"rayon-chunks"` are not. The user shouldn't see the backend name in any parameter.

### What is NOT in this plan, deliberately

- **Wire protocol design.** Premature. `rustlab-proto` (viewer IPC) was scoped tightly because we knew the viewer's needs; compute will have different per-call payload, batch granularity, and retry semantics. Designed when distributed work begins, in a separate plan.
- **Worker discovery / pool configuration.** Deployment-model-specific (local cluster vs. K8s vs. on-demand cloud). Cross that bridge later.
- **Failure recovery semantics.** Network partition, worker crash, partial results — answer depends on user tolerance.
- **`Value::FuncHandle` distributed semantics.** For local rayon, cloning the Evaluator carries user-defined functions trivially. For cluster mode, the worker needs the function definitions too — either ship them with each parmap call (option A, simple) or pre-load worker env from a script file (option B, more efficient for repeated calls). The user-facing API accepts `FuncHandle` in both modes; the wire-protocol decision picks A or B when distributed work starts.

### Phase 6 (deferred): cluster backend

Out of scope for this plan; placeholder reserved so the work has a known home when it starts. Will live as `dev/plans/rustlab_server.md` (or similar) and reference back to this plan's `ParmapBackend` trait. No design work happens until rustlab-server itself begins.

## Stats integration — how `parmap` composes with the stats toolkit

`parmap` is the **map** half of map-reduce; the rustlab stats functions are the **reduce** half. They're designed to compose, not compete. Three layers of integration, in increasing scope and decreasing certainty:

### Layer 1 — composition (no new work; already automatic)

Stats functions in rustlab today (`mean`, `median`, `std`, `var`, `sum`, `cumsum`, `prod`, `min`, `max`, `argmin`, `argmax`, `norm`, `trapz`, `dot`) all take a vector and return a scalar (or a vector for the cumulative variants). `parmap` returns a vector. They compose with zero glue code:

```rlab
estimates = parmap(@trial, 1:1000);
mu  = mean(estimates);
se  = std(estimates) / sqrt(length(estimates));
med = median(estimates);
```

This is the design. `parmap` and the stats library are complementary — `parmap` parallelises the work, the stats library aggregates the results. Nothing extra to build for the common Monte Carlo case.

Octave-comparison tests in `tests/octave/` already exercise the stats functions against Octave reference values; once `parmap` ships, the same stats functions land at the output of every MC run.

### Layer 2 — fused parallel-stats reductions (Phase 5-adjacent follow-on)

When `N` is large enough that the intermediate `parmap` result vector is itself a memory burden, fused builtins compute the statistic incrementally without ever materialising the full sample vector. Standard parallel-Welford accumulators give numerically stable mean / variance from one accumulator per rayon task, merged tree-style:

```rlab
mu          = parmean(@trial, 1:1_000_000);              % no Vec materialized
[mu, sigma] = parmean_std(@trial, 1:1_000_000);          % parallel Welford
sigma2      = parvar(@trial, 1:1_000_000);
[lo, hi]    = parminmax(@trial, 1:1_000_000);
```

Memory cost rules of thumb:
- Trials returning a scalar: a `Vec<f64>` of 10 M elements is 80 MB. `mean(parmap(...))` is fine; `parmean` is a minor optimisation.
- Trials returning a 100 × 100 matrix: 10 M results × 80 KB = 800 GB. Vector materialisation is impossible; `parmean` over the matrix output is **essential**.
- Trials returning a 1 000 × 1 000 sparse factor: same scale problem.

**Recommendation:** keep this layer out of the initial parmap plan. Build it as part of Phase 5 (`parreduce`) **only when a user hits the memory wall** — for typical scalar-output MC sweeps it's a noticeable-but-not-essential win, and the implementation needs care to get numerical stability right (the classic catastrophic-cancellation pitfalls in parallel variance combiners are real).

If/when built, the natural API set is: `parmean`, `parsum`, `parmean_std`, `parvar`, `parminmax`, `parquantile`. Implementation pattern is the same for each: parallel accumulators merged via `rayon::reduce` (or hand-rolled tree merge), one accumulator type per statistic (Welford for mean+var, ordered min/max pair, t-digest or P²-style estimator for quantiles).

### Layer 3 — statistical-resampling builtins (future, separate plan)

Higher-level statistical operations whose definition includes a parallelisable inner loop. These sit on top of `parmap` / `parreduce` as the primitive; they're user-facing API, not extensions of the parallelism layer:

```rlab
[mu, ci_lo, ci_hi] = bootstrap(@(s) mean(s), data, 10000, 0.95);
%                              statistic    B     confidence
[mu, sigma]        = jackknife(@(s) mean(s), data);
samples            = monte_carlo(@trial, 10000);           % parmap shorthand
[mu, ci]           = mh_sampler(@log_pdf, x0, 50000);       % Metropolis-Hastings
```

These are out of scope for the parmap plan because:
- Their API surface is large (CI conventions, percentile-vs-BCa-vs-studentized bootstrap methods, burn-in / thinning conventions for MH, etc.).
- They have correctness requirements parmap doesn't (the resampling itself, not just the parallelism).
- They don't change anything about how parmap is built — they just *use* it.

When this work begins it gets its own plan (likely `dev/plans/statistical_resampling.md`) that references back to the `parmap` primitive.

### Summary — what this plan bakes in for stats

| Layer | Where it lands | When |
|---|---|---|
| 1. Composition with existing stats | Automatic via Phase 2 | Day one of `parmap` shipping |
| 2. Fused `parmean` / `parvar` / `parsum` | Phase 5 sibling | When a user hits memory pressure on parmap output |
| 3. `bootstrap` / `jackknife` / resamplers | Separate plan | Future, on demand |

The current plan only needs Phase 2 + stats-as-they-exist; Layer 1 falls out for free. Layers 2 and 3 are referenced here so future agents don't accidentally redesign the integration story.

## Example user code — what success looks like

These are illustrative scripts the user-facing API should make easy. They drive the test suite for Phases 2–4 and the gallery notebook in Phase 4. The examples assume the `parmap` name; if the plan switches to `pararrayfun` (see "Naming alternative" in the design questions), substitute mechanically.

### 1. π by random sampling — the "hello world" of MC

```rlab
function p = pi_trial(k)
  N = 1_000_000;
  X = rand(N, 1) * 2 - 1;
  Y = rand(N, 1) * 2 - 1;
  p = 4 * sum(X.^2 + Y.^2 < 1) / N;
end

seed(42);                                  % deterministic across runs
estimates = parmap(@pi_trial, 1:8);        % 8 independent trials, parallel
print(mean(estimates))                     % → ~3.14159
print(std(estimates) / sqrt(length(estimates)))  % standard error
```

Key points the implementation must support:
- `@pi_trial` is a `Value::FuncHandle`, **not** a `Value::Lambda`. Both must work.
- `seed(42)` before parmap → bit-identical output across runs (Phase 3 contract).
- The lambda calls `rand` and `randn` — these go through the per-task RNG installed by parmap, so each trial sees a different stream.

### 2. Parameter sweep — Black-Scholes call price across spot prices

```rlab
function price = bs_call(S0)
  K = 100; r = 0.05; sigma = 0.2; T = 1.0; N = 500_000;
  Z = randn(N, 1);
  ST = S0 * exp((r - 0.5 * sigma^2) * T + sigma * sqrt(T) * Z);
  price = exp(-r * T) * mean(max(ST - K, 0));
end

spots = 80:2:120;                  % 21 spot values
seed(7);
prices = parmap(@bs_call, spots);  % each call ~50 ms

clf;
plot(spots, prices, "MC Black-Scholes call");
xlabel("S_0"); ylabel("call price");
```

Key points:
- The iterable here is `80:2:120`, a 1-D range of f64. parmap must accept colon ranges as iterables, not just `1:N` integer ranges.
- The result is a vector that drops straight into `plot()` — composability is part of the design.

### 3. Bootstrap confidence interval

```rlab
seed(1);
data = randn(1000, 1) * 2 + 5;       % normal, mean=5, sd=2

function m = boot_replicate(k)
  global data           % or capture via closure
  n = length(data);
  idx = randi(n, n, 1);
  m = mean(data(idx));
end

B = 10_000;
seed(99);
boot_means = parmap(@boot_replicate, 1:B);

ci_lo = sort(boot_means)(round(0.025 * B));
ci_hi = sort(boot_means)(round(0.975 * B));
print(sprintf("95%% CI for the mean: [%.3f, %.3f]", ci_lo, ci_hi))
```

Key points:
- The lambda reads `data` from the surrounding scope. The implementation must capture this — for `Lambda`, via the lambda's `captured_env`; for `FuncHandle`, via `global data` (already supported in rustlab).
- `B = 10_000` calls × ~µs per call is in "small per-call work" territory. Phase 4 may need the `"chunks"` knob (deferred design question 4 in the plan) to avoid per-task overhead.

### 4. Chunk-size override for very-light per-call work

```rlab
% A million tiny tasks would have terrible per-task overhead.
xs = 1:1_000_000;

% Naive: 1M parallel tasks → rayon's task scheduler dominates.
y = parmap(@(k) sin(k * 0.001), xs);                      % slow

% Batched: ~1000 elements per task, 1000 tasks total.
y = parmap(@(k) sin(k * 0.001), xs, "chunks", 1000);      % much faster
```

Key points:
- The `"chunks"` knob is a *deferred* design question (recommendation: not in the first cut). This example documents what it would look like if added.
- An anonymous `@(k) ...` lambda is in scope alongside named-function `@func` references.

### 5. Where `parreduce` would help (deferred Phase 5)

Most MC code is `mean(parmap(...))` — plain map plus sequential aggregation is enough. `parreduce` only matters when each trial returns something big and the combiner is non-trivial:

```rlab
function C = cov_trial(k)
  N = 5000;
  X = randn(N, 100);
  C = (X' * X) / N;     % returns a 100x100 matrix per trial
end

% With parmap alone: vec of 50 matrices, sum sequentially.
Cs = parmap(@cov_trial, 1:50);   % design question 3: matrix-per-call output shape?
C_avg = Cs{1};
for k = 2:length(Cs); C_avg = C_avg + Cs{k}; end
C_avg = C_avg / length(Cs);

% With parreduce: tree-shaped combine in parallel.
C_sum = parreduce(@plus, zeros(100, 100), parmap(@cov_trial, 1:50));
C_avg = C_sum / 50;
```

Key points:
- This example is intentionally awkward under plain `parmap` — the matrix-per-call output shape is open design question 3. The Phase 1 implementation should hard-error on matrix-per-call output and document the limitation.
- `parreduce` is *deferred* until a real user case demands it. This example shows the shape it would take when revisited.

### 6. `nproc()` — machine introspection (Phase 4)

```rlab
n = nproc();
print(sprintf("running on %d threads", n))

% Adaptive: skip parmap overhead on a single-core box
if n >= 4
  results = parmap(@expensive_trial, 1:1000);
else
  results = zeros(1, 1000);
  for k = 1:1000
    results(k) = expensive_trial(k);
  end
end
```

Key points:
- `nproc()` returns the same number rayon's global pool will use (logical cores, respecting cgroup limits on Linux).
- Cheap to call; safe to call inside scripts, REPL, or notebooks. No worker threads spawned.
- Pairs well with `sprintf` for runtime diagnostics ("running on 12 threads, 1000 trials").

### 7. Distributed backend — same script, different deployment (Phase 6 deferred)

The future cluster backend lands without changing user-facing scripts. Configuration happens once at startup; every subsequent `parmap` call dispatches through the configured backend automatically.

```rlab
% --- Default: local rayon pool (what Phase 2 ships) ---
estimates = parmap(@trial, 1:10000);              % 12 threads on this M2 Pro
print(mean(estimates))

% --- Future cluster mode: same script, different config ---
% Config provided once via env var: RUSTLAB_CLUSTER_POOL="host1:9000,host2:9000,host3:9000"
% Or via a startup builtin (TBD when Phase 6 is planned):
%   parmap_use_cluster("host1:9000", "host2:9000", "host3:9000");
estimates = parmap(@trial, 1:10000);              % dispatched across 3 worker hosts
print(mean(estimates))

% Hybrid: explicit local-only override on a per-call basis (not in v1)
%   estimates = parmap(@trial, 1:10000, "backend", "local");
```

Key points:
- The script body is **literally identical** between modes. Only the configuration (env var or one-time builtin) differs.
- The pure-lambda contract from Phase 3 is exactly what makes this safe: anything the contract allows is also wire-serializable.
- `Value::FuncHandle` (e.g., `@trial` referring to a `function r = trial(k); ...; end`) works in both modes; in cluster mode, the user's function definitions ship with the parmap call (Phase 6 design decision).
- The result `mean(estimates)` runs on the calling machine — only the per-trial work distributes. Stats reduction stays local unless the user explicitly calls a `parmean`-style fused reducer (Layer 2 of the stats integration section).

### 8. Fused parallel-stats reducer — `parmean` (Phase 5 deferred)

When trials return scalars and the sample vector is small, plain `mean(parmap(...))` is fine. When trials return large objects (matrices, sparse factors) or `N` is very large, the intermediate `Vec<Value>` becomes a memory burden — `parmean` and friends would compute the statistic incrementally:

```rlab
% Plain map-then-reduce: materializes a 1M-element Vec, then averages.
% For scalar trials this is fine — 8 MB.
estimates = parmap(@scalar_trial, 1:1_000_000);
mu = mean(estimates);

% Fused: never materializes the vector. Welford's algorithm in parallel —
% one accumulator per rayon task, merged tree-style at the end.
mu = parmean(@scalar_trial, 1:1_000_000);

% Returns both mean and stddev in one pass (numerically stable):
[mu, sigma] = parmean_std(@scalar_trial, 1:1_000_000);

% Critical for big-output trials. A trial returning a 100×100 covariance
% matrix can't be stored 1M times (800 GB of intermediates). parmean
% accumulates element-wise into a single 100×100 running mean.
C_avg = parmean(@cov_trial, 1:1_000_000);
```

Key points:
- Reduces peak memory from O(N · sizeof(trial output)) to O(sizeof(trial output)).
- Numerical stability requires care — Welford's algorithm (or Chan's parallel variant for the combine step) handles the catastrophic-cancellation pitfalls that naive `sum(x) / n; sum(x.^2) / n - mean^2` fails on.
- Recommended for Phase 5 *only* when a user hits the memory wall. For typical scalar-output Monte Carlo, plain `parmap` + sequential `mean` is sufficient and easier to reason about.

### 9. The pure-lambda contract — what `parmap` deliberately won't do

Animations and any per-frame I/O go in the **main thread**, not under parmap. The pure-lambda contract (Phase 3) hard-errors if a parallel lambda calls `savefig`, `clf`, `fprintf`, etc.

```rlab
% Doesn't work — the lambda touches plot/file state.
parmap(@(t) (clf(); plot(...); savefig(sprintf("frame_%d.svg", t))), 1:60)
%                                         ^^^^^^^
%   error: parmap: cannot savefig from a parallel lambda — the lambda must be pure

% Does work — split compute (parallel) from I/O (serial).
frames = parmap(@(t) build_frame_data(t), linspace(0, 2*pi, 60));
for k = 1:60
  imagesc(frames{k});
  savefig(sprintf("frame_%03d.svg", k));
end
```

Key points:
- The error message must mention both `parmap` and the offending builtin name. Tests in Phase 3 enforce this (named tests: `parmap_clf_errors`, `parmap_fprintf_errors`, `parmap_savefig_errors`).
- The split-compute-from-IO idiom is documented in the gallery notebook (Phase 4) so users learn the pattern alongside the feature.

## Six mandatory workflow rules (apply to every phase)

Per `feedback_workflow.md` and `AGENTS.md`:

1. **Plan first** — this doc *is* the plan. Per-phase tweaks need user approval if they change scope.
2. **Tests in the same commit** — algorithm/contract tests in `crates/rustlab-script/src/tests.rs`. Run `cargo test --workspace` *and* `cargo test --workspace --features viewer` before declaring done.
3. **No commit without explicit approval** — present a summary, wait for "commit" / "push".
4. **Update `AGENTS.md`** function table for `parmap` (and `parreduce` when it lands).
5. **Update `docs/quickref.md`** in the relevant section ("Lambdas / Higher-Order" or a new "Parallelism" section).
6. **Update REPL help** — `HelpEntry { name, brief, detail }` for each new builtin in `crates/rustlab-cli/src/commands/repl.rs`, and add to the relevant `categories` slice in `print_help_list`.

A feature is **not done** until `help parmap` returns useful text.

## Verified file landmarks (re-verify if more than ~14 days old; this list captured 2026-05-10)

- `crates/rustlab-script/src/eval/mod.rs` — `pub struct Evaluator` at line 29 (env, builtins, user_fns, profiler, in_function flag, color_output, number_format, current_line). `fn eval_lambda_call` at line 1834 — current shape is `&mut self`, swaps `self.env` for `captured_env`. This is what `parmap` needs to invoke per-element.
- `crates/rustlab-script/src/eval/builtins.rs` — `pub type BuiltinFn = fn(Vec<Value>) -> Result<Value, ScriptError>` at line 28; `BuiltinRegistry` at line 41. Builtins do **not** receive `&mut Evaluator` today, which is why `parmap` cannot be a normal builtin — it must live as a higher-order operator the evaluator dispatches specially.
- `crates/rustlab-script/src/eval/value.rs` — `pub enum Value` (variants: Scalar, Complex, Vector, Matrix, Tensor3, Bool, Str, Lambda, FuncHandle, FirState, AudioIn, AudioOut, LiveFigure, SparseVector, SparseMatrix, SparseFactor, …). `Lambda { params, body, captured_env }`. `FuncHandle(String)`.
- `crates/rustlab-script/src/eval/profile.rs` — `pub struct Profiler` derives `Clone`, `Default`. Easy to clone.
- `Cargo.toml` (workspace root) — `rayon = "1.10"` already in `[workspace.dependencies]` (added during em_performance Phase 3).
- `crates/rustlab-script/Cargo.toml` — does **not** yet depend on rayon. Phase 1 adds it.

## Design questions — answers locked (2026-05-11)

All seven open questions resolved. The user approved the plan-level scope and all seven recommendations on 2026-05-11. Future agents: do not re-litigate.

1. **Scope of accepted callable types.** `Lambda` (`@(k) …`) **and** `FuncHandle` (`@my_user_function`). Both common in MC. Excluded: builtin-by-name (`parmap("sin", xs)`) — keep the call site explicit and consistent with how higher-order operators read elsewhere in rustlab.

2. **Iterable shape.** **1-D only in v1.** Vectors, row-vectors, and colon ranges (`1:N`, `0:0.1:10`). Matrix-column parmap is a follow-on if a use case appears. Octave's `arrayfun` accepts matrices but the rustlab v1 cut keeps the iteration model unambiguous.

3. **Output shape.** **Scalar-return → `Value::Vector`.** Vector- or matrix-return: **error in v1** with a clear message ("parmap: lambda must return a scalar; got vector"). Matrix-stacking and cell-array-style return shapes deferred to a follow-on.

4. **Default thread count.** **No `"threads"` knob in v1.** rayon's global pool sized by `std::thread::available_parallelism()` is the v1 strategy. The new `nproc()` builtin (Phase 4) lets users *see* the count for diagnostics.

5. **Error semantics.** **Cancel + propagate.** First task to error short-circuits the parmap; the error message identifies which trial failed (`parmap: trial 47 of 1000 errored: division by zero`). Matches `for` loop semantics; matches what users intuit from `arrayfun` analogues.

6. **Pure-lambda contract enforcement.** **Hard error**, not warning. Silent-wrong is worse than loud-fail. The error message names both `parmap` and the offending builtin (`parmap: cannot savefig from a parallel lambda — the lambda must be pure`).

7. **Profiler interaction.** **Disable inside parmap**, document the limitation. The script-layer profiler's higher-order-suppression machinery (`enter_higher_order` / `exit_higher_order`) already exists for similar cases (lambdas called from `arrayfun` precedents); reuse it.

## Cross-cutting test fixtures

Reusable across phases:

- **Trivial parmap:** `parmap(@(k) k^2, 1:10)` → `[1, 4, 9, ..., 100]`. Covers the happy path.
- **Larger range:** `parmap(@(k) sin(k * pi / 100), 1:1000)`. Verifies parallel execution for a meaningful workload.
- **Determinism:** `seed(42); a = parmap(@(k) randn(1, 1), 1:50); seed(42); b = parmap(...)`; assert `a == b` bit-exactly. Tests per-task seeding.
- **Pure-contract violation:** `parmap(@(k) (clf(); k), 1:5)` → runtime error mentioning `parmap` and `clf`. Tests the contract enforcement.
- **Error in one trial:** `parmap(@(k) 1/k, 0:5)` → cancel + propagate division-by-zero error. Tests the error semantics decision.

---

## Phase 1 — `Evaluator: Clone + Send`, AST/Value serializable

**Status:** shipped — commit `20d4a82` (2026-05-11)

**Implementation log (2026-05-11):**
- Added `#[derive(Clone)]` to `Evaluator`, `Profiler`, and `BuiltinRegistry`. UserFn was already Clone; the AST tree (Stmt, StmtKind, Expr, BinOp, UnaryOp) was already Clone.
- Added a compile-time `_assert_send::<Evaluator>` + `_assert_send::<Value>` at the end of `eval/mod.rs`. Catches non-Send regressions at compile time.
- Added `#[derive(serde::Serialize, serde::Deserialize)]` to every node in the AST (`Stmt`, `StmtKind`, `Expr`, `BinOp`, `UnaryOp`).
- Added `serde` (with `derive` + `rc` features) as a production dep on `rustlab-script`; enabled the `serde` feature on `num-complex` and `ndarray`. Added `rmp-serde` as a dev-dep.
- Rescoped: dropped Value-level serde and the serialize overhead bench from Phase 1. The cross-crate audit on `rustlab-core` (~30 types) is too big for Phase 1's deliverable; deferred to Phase 2 where the bench result will also decide whether the local backend needs serialization at all.
- 5 new tests in `tests::parmap_phase1_foundation`: `evaluator_is_send`, `clone_evaluator_preserves_env_and_user_fns`, `cloned_evaluator_runs_user_fn_independently`, `ast_expr_round_trips_through_msgpack`, `ast_stmt_round_trips_through_msgpack`. All pass.
- Workspace pass: 1738 tests, 0 failures. `--features viewer` also clean.
**Goal:** make `Evaluator` cloneable + Send (so rayon workers can carry their own copy) **and** make the AST + pure `Value` variants `Serialize + Deserialize` (so the same Lambda + captured env can later cross a network in distributed mode). Doing both audits in one phase saves a future cross-cutting refactor — see "Future compatibility" above.

**Scope:**

**Clone + Send (single-machine prerequisite):**
- Add `#[derive(Clone)]` on `Evaluator`. Audit each field:
  - `env: HashMap<String, Value>` — clone needs `Value: Clone`. Already true.
  - `builtins: BuiltinRegistry` — `HashMap<String, BuiltinKind>` with `BuiltinKind: Copy`. Derive Clone if not already.
  - `user_fns: HashMap<String, UserFn>` — clone needs `UserFn: Clone`. Verify.
  - `profiler: Profiler` — already Clone.
  - simple flags / counters — Copy.
- Verify `Value` variants are all `Send`. The `Arc<Mutex<…>>` variants (FirState, LiveFigure) are Send because Mutex<T: Send> is Send. The `Box<Expr>` in `Lambda` is Send if `Expr: Send`. Audit `Expr` recursively.
- Compile-time assertion: `fn assert_send<T: Send>() {}` for `Evaluator` and `Value`.

**Serde derives — AST only in Phase 1; Value-level deferred:**
- Add `#[derive(Serialize, Deserialize)]` on the AST: `Expr`, `Stmt`, `StmtKind`, `BinOp`, `UnaryOp`. Mostly mechanical, contained in a single file. **This is what Phase 1 actually ships.**
- **Value-level serde is moved to Phase 2.** Originally planned for Phase 1 but rescoped 2026-05-11 once the cross-crate audit revealed that ~30 types in `rustlab-core` (`SparseMat`, `SparseVec`, `SparseCsc`, `SparseChol`, `SparseLU`, `Permutation`, `OrderingHint`, …) need serde derives first. That's a real audit, not a one-file derive, and it's premature for Phase 1's deliverable — the AST is what proves "lambda bodies can cross a wire," and the captured environment portion can be tackled at Phase 2 time alongside the `ParmapBackend` trait, by which point we'll also know from the bench whether the local backend even needs to serialize.
- When Phase 2 picks this up, the choice is: (a) add serde to `rustlab-core` types and define `SerializableValue` as a parallel enum to `Value` covering only the pure variants, or (b) implement custom `Serialize`/`Deserialize` on `Value` itself that errors on stateful variants. The plan's original intent was (b); the audit will decide.
- Add `rmp-serde` to `[dev-dependencies]` for the Phase 1 AST round-trip test; production dep only if Phase 2 actually needs it (deferred decision).

**Cost-of-clone caveat:** cloning the full Evaluator copies all user-defined functions and the env. For a `parmap` over `N` elements, we don't want N full clones — we want one clone per *thread*, not per *task*. Phase 2's implementation has to use rayon's per-thread caching (thread-local) or a `Mutex<Vec<Evaluator>>` worker pool to amortize.

**Forcing-function discipline (for confirmation during Phase 1):** the "serialize even in the local backend" idea — see "Future compatibility" point 4 — is principled but may add non-trivial overhead per call on small Values. Phase 1's task: measure the round-trip cost on representative payloads (Scalar, 100×100 matrix, sparse 1000×1000 matrix). If the overhead is sub-microsecond on Scalar and sub-millisecond on the matrix cases, ship the discipline. If not, drop it for the local backend and rely on the pure-lambda contract alone.

**Files affected:**
- `crates/rustlab-script/src/eval/mod.rs` — derive Clone on `Evaluator`.
- `crates/rustlab-script/src/eval/builtins.rs` — derive Clone if needed.
- `crates/rustlab-script/src/eval/value.rs` — derive Serialize/Deserialize on pure variants; explicit error impl on stateful ones; Send assertion.
- `crates/rustlab-script/src/ast.rs` — derive Serialize/Deserialize on the whole AST tree; verify Send.
- `crates/rustlab-script/Cargo.toml` — add `serde` (workspace dep) and `rmp-serde` (workspace dep). Both are already in the workspace.

**Tests (Phase 1 scope, post-rescope):**
1. `evaluator_is_send` — compile-time assertion via `fn _assert_send<T: Send>()`.
2. `clone_evaluator_preserves_env_and_user_fns` — clone, mutate the clone, assert original is unchanged.
3. `cloned_evaluator_runs_user_fn_independently` — define a function in the original, clone, call from the clone, no shared state crosstalk.
4. `ast_expr_round_trips_through_msgpack` — serialize a representative `Expr` tree via `rmp-serde`, deserialize, compare structurally. Covers the body half of a Lambda.
5. `ast_stmt_round_trips_through_msgpack` — same for `Stmt`. Function bodies and complex control flow.

Deferred to Phase 2:
- `lambda_round_trips_through_msgpack` (needs Value serde).
- `pure_value_variants_round_trip_through_msgpack` (needs core-types serde).
- `stateful_value_variants_serialize_errors` (needs the custom impl).
- `serialize_overhead_benchmark` (decides forcing-function discipline; happens at Phase 2 boundary).

**Acceptance (Phase 1 scope, post-rescope):**
- All existing tests still pass.
- `Evaluator: Clone + Send` compiles.
- AST nodes round-trip through `rmp-serde` to bit precision.
- The `Value` enum is *not* serializable yet — that's deferred to Phase 2 with the choice between (a) parallel `SerializableValue` enum or (b) custom impl on `Value` to be made then. Both options preserve the user-facing API; the choice is internal.

**Risk:** medium-high (up from medium). The Send audit is the same low-risk pass as before. The serde derives add real risk: (a) the AST has 30+ variants, mostly fine but the `Box<Expr>` recursion needs `serde(rec)` or similar; (b) the `Value` enum has nested types from `ndarray` (`CMatrix = Array2<C64>`) which need `ndarray`'s `serde` feature enabled (it has one — `ndarray = { workspace, features = ["serde"] }`); (c) the `Sparse*` types and `SparseFactor` need their own derives, and `SparseFactor` wraps `Arc<SparseChol>` which needs `Arc` to be serializable (it is — serde's `rc` feature, off-by-default on Arc but easy to enable).

**Estimated size:** ~80 LoC Clone work + ~150 LoC serde derives + ~120 LoC tests + bench. Three-quarters of a session if all the audits go smoothly; one-and-a-half if `ndarray` or `Arc` serde wiring is finicky.

---

## Phase 2 — `parmap(f, xs)` builtin behind a backend trait

**Status:** shipped — commit `98d084c` (2026-05-11)

**Implementation log (2026-05-11):**
- New module `crates/rustlab-script/src/eval/parmap.rs` (~190 LoC). Defines `ParmapBackend` trait, `LocalRayonBackend` impl, `validate_callable` helper, and `pack_results` packer. Each is small on purpose — the trait surface stays minimal so a future cluster backend (Phase 6) is purely additive.
- Added `rayon.workspace = true` to `crates/rustlab-script/Cargo.toml`.
- Made the existing `Evaluator::call_callable` (Lambda + FuncHandle dispatch) `pub(crate)` so `eval_parmap` can use it directly instead of inventing a parallel callable enum.
- Added `Evaluator::clone_for_parallel_lambda` as a thin wrapper around `Clone::clone` — semantic name for the future when per-worker trimming might happen.
- Added compile-time `_assert_sync::<Evaluator>` + `_assert_sync::<Value>` alongside the existing Send assertions. Both pass.
- Added `eval_parmap` to `Evaluator` impl, modeled on `eval_arrayfun`: validates callable, extracts iterable elements (1-D vector / scalar / complex scalar), clones the Evaluator as a template, hands off to the backend, packs results. Profiler timing wired in.
- `Call("parmap", [f, xs])` dispatched in `eval_expr` alongside `arrayfun` / `rk4` / `feval`.
- 8 new tests in `tests::parmap_phase2_dispatch`: lambda squares (`1..10`), function-handle dispatch, colon-step range, parmap-vs-arrayfun bit-identity on a smooth lambda, errors on non-callable, errors on non-iterable, error propagation from one trial, complex-valued lambda preserves complex output.
- Workspace: 1750 tests pass (+12 over Phase 1's 1738). `--features viewer` clean.
- End-to-end smoke test against the REPL: `parmap(@(k) k^2, 1:10)` returns `[1, 4, 9, …, 100]`; `parmap(@trial, 1:5)` for a user-defined `trial` works.

**Deferred to Phase 3 (still pending):**
- Per-task RNG seeding from a master seed (Monte Carlo determinism).
- Pure-lambda contract enforcement (hard-error on `clf`, `fprintf`, `savefig`, etc. inside a parallel lambda).
- Profiler-inside-parmap suppression (currently a lambda call inside parmap will record per-call profiling; should be suppressed via the existing `enter_higher_order` / `exit_higher_order` machinery).

**Per-thread caching:** the v1 implementation clones the Evaluator template once per element (N clones for N elements). For typical Monte Carlo this is sub-millisecond overhead per call. Per-thread caching (one clone per rayon worker, shared across all tasks landing on it) is a follow-on if profiling demands it.

**Value-level serde (deferred from Phase 1):** still deferred. Phase 6 (cluster backend) is the trigger; Phase 2's local backend doesn't need it (Evaluator clones happen in shared memory, no wire boundary).
**Goal:** introduce the higher-order `parmap` operator behind a backend abstraction so the future cluster backend lands additively. Phase 2 ships the `LocalRayonBackend` impl as the only concrete backend; the trait surface is small but locked in here.

**Scope:**

**Backend trait (forward-compat for Phase 6 cluster):**
- New module `crates/rustlab-script/src/eval/parmap.rs`. Defines:
  ```rust
  pub trait ParmapBackend: Send + Sync {
      fn run(
          &self,
          worker_factory: &dyn Fn() -> Evaluator,
          lambda: &CallableValue,        // Lambda or FuncHandle, normalised
          xs: &[Value],                  // already evaluated input elements
          master_seed: u64,              // for per-task RNG (Phase 3)
      ) -> Result<Vec<Value>, ScriptError>;
  }
  ```
  Trait is small on purpose. Worker factory closure lets the local backend lazy-clone Evaluators per-thread; the cluster backend ignores it (workers are remote).
- `LocalRayonBackend` impl: uses rayon's global pool, thread-local Evaluator clones, calls `eval_lambda_call` per element. The only impl shipped in Phase 2.
- The `parmap` dispatch in `eval_expr` constructs a `LocalRayonBackend` and calls through the trait. Selection of which backend to construct is a stub today (always returns `LocalRayonBackend::new()`); the future cluster backend will plug in here with config-driven selection.

**Dispatch at the call site:**
- Special-case dispatch in `Evaluator::eval_expr` for `Call("parmap", [f_expr, xs_expr])`. Cannot use the normal `BuiltinRegistry` path because builtins don't receive `&mut Evaluator`.
- Evaluate `f_expr` to a `Value::Lambda` or `Value::FuncHandle`. Error otherwise.
- Evaluate `xs_expr` to a 1-D `Value::Vector`. Error otherwise.
- Construct the chosen backend (always `LocalRayonBackend` in Phase 2).
- Build a worker factory: `|| self.clone_for_parallel_lambda()`. Each rayon worker thread caches one Evaluator from this factory.
- Call `backend.run(...)`. Collect results.

**Pack the result:**
- If all results are scalars, return `Value::Vector`. If complex scalars, same.
- Mixed scalar/complex types — promote to complex. Mixed scalar/matrix — error (defer to a follow-on).

**Forcing-function discipline (decided in Phase 1):**
- If Phase 1's bench showed serde overhead is acceptable, the local backend serializes-and-deserializes the input element + result Value at the worker boundary even in single-machine mode. Discipline: keeps the cluster path's bug surface aligned with the local path's.
- If the bench showed it's too costly, skip — the local backend works on owned `Value` directly and the cluster impl in Phase 6 carries the serialization cost itself.

**Files affected:**
- `crates/rustlab-script/src/eval/mod.rs` — special-case in `eval_expr`'s `Call` branch; helper `clone_for_parallel_lambda`.
- `crates/rustlab-script/Cargo.toml` — add `rayon.workspace = true`.
- `crates/rustlab-script/src/eval/parmap.rs` (new) — `ParmapBackend` trait, `LocalRayonBackend` impl, helpers.

**Tests:**
1. `parmap_squares` — `parmap(@(k) k^2, 1:10)` → `[1, 4, …, 100]`. Bit-exact.
2. `parmap_complex_lambda` — `parmap(@(k) sin(k * 0.1), 1:100)`. Compare against sequential `arrayfun`-equivalent sequential `for` loop.
3. `parmap_func_handle` — define `function r = trial(k); r = k + 1; end`, then `parmap(@trial, 1:5)` → `[2, 3, …, 6]`.
4. `parmap_errors_on_non_callable` — `parmap(42, 1:5)` → clear error.
5. `parmap_errors_on_non_iterable` — `parmap(@(k) k, 42)` → clear error.
6. `parmap_propagates_errors` — `parmap(@(k) 1/k, 0:5)` → error from k=0 trial bubbles up.
7. `parmap_actually_uses_threads` — measure wall time of `parmap(@(k) sleep_ms(50), 1:8)` on an 8-core machine; expect `< 200ms`. (Skip in CI if flaky.)

**Acceptance:**
- All tests pass.
- Wall-time on a benchmark `parmap(@(k) heavy_compute(), 1:N)` shows ~`N_cores`× speedup over `arrayfun` (no `arrayfun` in rustlab today; compare against a sequential `for` loop).

**Risk:** medium. Main risks: (a) per-thread Evaluator cloning blows the cost budget — mitigation: amortize via rayon's thread-local; (b) error propagation has subtle ordering — mitigation: collect into `Result<Vec<Value>, _>` and return the first Err; (c) some `Expr` node type turns out to be Send-unfriendly when stored across threads — mitigation: caught by Phase 1.

**Estimated size:** ~250 LoC implementation + ~150 LoC tests. One session.

---

## Phase 3 — Per-task RNG + pure-lambda contract

**Status:** shipped — commit `0d789c4` (2026-05-11)

**Implementation log (2026-05-11):**

Per-task RNG seeding:
- Added a `MASTER_SEED: Cell<Option<u64>>` thread-local in `eval/rng.rs` next to the existing `RNG`. `seed_rng(N)` now records `N` here; `seed_rng_from_entropy()` clears it.
- New `current_master_seed() -> Option<u64>` reads the current value for parmap dispatch.
- New `derive_task_seed(master, idx) -> u64` mixes the two via SplitMix64 finalizer. Deterministic and avalanching.
- `eval_parmap` reads `current_master_seed()` on the calling thread; if `None`, draws a single u64 from OS entropy *without* touching the master RNG, so the master is undisturbed regardless of whether `seed(N)` was called.
- `LocalRayonBackend::run` now `seed_rng(derive_task_seed(master, idx))` at the start of each rayon task. Determinism contract: `seed(N); parmap(...)` is bit-reproducible across runs; the calling thread's master RNG state is unchanged after parmap returns.

Pure-lambda contract enforcement:
- New `PARALLEL_CONTEXT: Cell<bool>` thread-local in `eval/parmap.rs`.
- `ParallelContextGuard` RAII wrapper sets the flag on enter, restores on drop. Installed once per rayon task in `LocalRayonBackend::run`.
- New `require_pure_context(builtin_name)` returns `Err(...)` when called from a worker task with the standard error message: `parmap: cannot {name} from a parallel lambda — the lambda must be pure`.
- New `IMPURE_BUILTINS` const list in `eval/mod.rs` (32 names: plotting, file I/O, audio, FirState, seed). `Evaluator::call_builtin_tracked_nargout` checks this list at entry and calls `require_pure_context` for any match. One choke point catches every call path through the builtin registry.

8 new tests in `tests::parmap_phase3_correctness`:
- `parmap_deterministic_with_master_seed` — bit-identical output for the same seed.
- `parmap_master_seed_unchanged_after` — post-parmap master RNG state matches no-parmap baseline.
- `parmap_per_task_rng_is_independent` — 50-trial uniqueness sanity check.
- `parmap_clf_errors`, `parmap_fprintf_errors`, `parmap_savefig_errors` — contract enforcement.
- `parmap_seed_inside_lambda_errors` — seed itself is banned inside parmap.
- `parmap_nested_recursive_works` — nested parmap is allowed; per-task seeds derive correctly through the recursion.

## Phase 4 — Tests, docs, REPL help

**Status:** shipped — commit `0d789c4` (2026-05-11)

**Implementation log (2026-05-11):**
- New `nproc()` builtin in `eval/builtins.rs`. Returns `std::thread::available_parallelism()` (or 1 on fallback). Same number as rayon's pool size.
- REPL `HelpEntry` records for `parmap` and `nproc` added next to `arrayfun` in `crates/rustlab-cli/src/commands/repl.rs`.
- New `"Parallelism"` category in `print_help_list` containing `parmap` + `nproc`.
- `AGENTS.md` function table updated with a `parmap` row and an `nproc` row; the existing "Higher-order" line now mentions both arrayfun and parmap.
- `docs/quickref.md` Language section gained two rows (`parmap`, `nproc`) right after `arrayfun`.
- New gallery notebook `examples/notebooks/parallel_montecarlo.md`: π by random sampling, Black–Scholes parameter sweep, and a worked example of the pure-lambda contract. Renders cleanly (5 code blocks, 2 plots, 0 errors).
- Fixed a doctest false-positive: the example line in `require_pure_context`'s comment was being interpreted as Rust source by rustdoc; wrapped in a ```text fence.

Workspace: 1758 tests pass (+8 over Phase 2's 1750), 0 failures. Gallery re-baked, all 31 notebooks render.
**Goal:** make `parmap` correct for Monte Carlo — each task gets its own RNG state — and enforce the pure-lambda contract at runtime.

**Scope:**

**Per-task RNG.** Today `seed(N)` sets a process-global RNG. In a parmap, each task needs its own RNG state derived from the master seed:
- Pre-`parmap`: master seed is whatever `seed()` last set (or process-default if never set).
- Inside `parmap`: each task computes its seed as `hash(master_seed, task_index)` (use `siphash` or any fast deterministic mix). Each task installs that seed on its thread-local Evaluator's RNG before calling the lambda.
- The Evaluator's RNG needs to be thread-local for this. Currently it's process-global (`thread_rng()` — actually thread-local already). Verify and document.
- After parmap completes, the master RNG is unchanged (the parallel work didn't perturb it).

**Pure-lambda contract enforcement.** In each thread-local Evaluator used inside parmap, install a `parallel_context: bool` flag. Builtins that touch global state (plot, file I/O, audio, FirState) check the flag at entry and error with a specific message:
- `clf`, `figure`, `plot`, `imagesc`, `quiver`, `streamplot`, `contour`, `surf` etc. — "parmap: cannot plot from a parallel lambda — the lambda must be pure"
- `fprintf`, `fopen`, `fclose`, `csvwrite`, `csvread`, `savefig` — same with the offending name
- `AudioOut` writes — same
- `FirState` mutation — same

The check is a one-liner per affected builtin: `if ctx.is_parallel() { return Err(...); }`. Implementation: pass the parallel flag into the BuiltinRegistry's call shape, OR install a thread-local flag the builtins consult.

**Files affected:**
- `crates/rustlab-script/src/eval/parmap.rs` — drive per-task seeding; install parallel flag.
- `crates/rustlab-script/src/eval/builtins.rs` — add the contract checks at the entry of each impure builtin. About 15 affected functions; the check itself is a single function call.
- `crates/rustlab-script/src/eval/mod.rs` — store the parallel flag as a thread-local or as an Evaluator field cloned with `parallel_context: true` for parmap workers.

**Tests:**
1. `parmap_deterministic_with_master_seed` — `seed(42); a = parmap(@(k) randn(1, 1), 1:50); seed(42); b = parmap(...)`; assert `a == b` bit-exactly.
2. `parmap_master_seed_unchanged_after` — `seed(42); _ = parmap(@(k) randn(1, 1), 1:10); x = randn(1, 1)` produces the same `x` as if no parmap ran.
3. `parmap_per_task_rng_is_independent` — each task gets a different RNG stream (with high probability). Histogram-or-uniqueness sanity check.
4. `parmap_clf_errors` — `parmap(@(k) (clf(); k), 1:3)` → clear error mentioning `clf` and `parmap`.
5. `parmap_fprintf_errors` — same for `fprintf`.
6. `parmap_savefig_errors` — same for `savefig`.

**Acceptance:**
- Determinism test passes on every run.
- Contract-violation tests produce the expected error string.
- Real Monte Carlo example (the user's actual case, when shared) gives the same answer (within statistical noise) as the sequential equivalent.

**Risk:** low–medium. The seeding logic is straightforward. The contract enforcement is mechanical but spread across many builtins — risk of missing one. Mitigation: a single helper function `assert_pure_context("clf")?` that all impure builtins call as their first line; auditable via grep.

**Estimated size:** ~150 LoC + ~120 LoC tests. Half a session.

---

## Phase 4 — Tests, docs, REPL help

**Status:** pending
**Goal:** the workflow-rule completion items.

**Scope:**
- Update `AGENTS.md` function table — new row for `parmap`.
- Update `docs/quickref.md` — new section ("Parallelism" or under "Lambdas / Higher-Order").
- Update `docs/functions.md` if it has detailed entries for similar features.
- REPL `HelpEntry` for `parmap` plus new "Parallelism" category in `print_help_list`.
- A small `gallery/` notebook or `examples/notebooks/` notebook demonstrating Monte Carlo (e.g., π via random sampling, with parmap vs sequential comparison).

**Files affected:**
- `AGENTS.md`
- `docs/quickref.md`
- `docs/functions.md` (if applicable)
- `crates/rustlab-cli/src/commands/repl.rs`
- `examples/notebooks/parallel_montecarlo.md` (new, optional)

**Risk:** low.

**Estimated size:** ~150 LoC of doc + the notebook. Quarter-session.

---

## Phase 5 — `parreduce(f, init, xs)` (deferred)

**Status:** deferred — only build if a concrete use case demands it.

**Goal (when revisited):** parallel fold. Take a binary function `f(a, b)`, an identity `init`, and an iterable `xs`. Return `f(init, f(xs[0], f(xs[1], …)))` computed in a tree shape via rayon.

**When to revisit:** if a user case appears where:
- Each element is expensive to compute (so paralleism matters).
- The final answer is a single scalar / small struct (so reduction is meaningful).
- Plain `parmap` followed by `sum`/`mean`/`max` is not enough (e.g., needs a custom combiner).

For typical Monte Carlo where each trial returns a scalar and aggregation is `mean(parmap(...))`, plain `parmap` + sequential aggregation is sufficient. `parreduce` is an optimization for the case where the parmap result vector itself is large.

**Estimated size when built:** ~150 LoC + ~80 LoC tests. Same week as Phase 2 if combined.

---

## Phase 6 — Cluster backend (deferred, separate plan)

**Status:** deferred. Placeholder; no scope work happens here.

**Trigger:** when work begins on `rustlab-server` for distributed compute.

**Where it lives:** a new plan file under `dev/plans/rustlab_server.md` (or whatever the server work is called) that references back to this plan's `ParmapBackend` trait. The cluster backend is purely additive — it implements the trait, plugs into the dispatch's backend-selection stub from Phase 2, and changes nothing user-facing.

**Expected scope (for reference, not a commitment):**
- Wire protocol for compute traffic (`rustlab-proto-compute` extension or sibling of the existing `rustlab-proto` viewer protocol).
- Worker discovery / pool configuration (env var, config file, builtin like `parmap_use_cluster("host1", "host2", …)`).
- `Value::FuncHandle` distributed semantics — ship user-fns with each parmap call (option A from "Future compatibility"), or pre-load worker env from a script (option B). Decision deferred.
- Failure recovery semantics: retry, partial-result, fail-fast. Use-case-driven.
- Distributed RNG seeding: same `(master_seed, task_index) → hash` rule as the local backend. No new design needed.
- Per-host nproc-aware scheduling.

**What's already done now (in Phase 1) to enable it:**
- AST + pure `Value` variants are `Serialize + Deserialize`.
- `Lambda` round-trips through `rmp-serde` (already tested).
- `ParmapBackend` trait is in place; cluster impl is purely additive.
- Pure-lambda contract bounds the wire payload — only serializable `Value` variants can cross the boundary.

## Phase ordering and risk gates

Phase 1 is the foundation. If `Evaluator: Clone + Send` doesn't go cleanly, the whole plan needs a rethink (either find the non-Send type and fix it, or fall back to a subprocess-based parmap). **Do not start Phase 2 until Phase 1's Send assertion compiles.**

Phase 2 produces a usable `parmap` but with no per-task seeding. Functional but Monte Carlo-incorrect.

Phase 3 closes the correctness gap. After this, `parmap` is shippable.

Phase 4 is the doc/help workflow rule satisfaction.

Phase 5 stays deferred until a concrete use case appears.

## What's *not* in this plan

- **`parfor` syntax.** Decided against in the design conversation — adds parser surface for no expressive gain over `parmap`.
- **Async / futures / background tasks.** Different problem (I/O concurrency, not CPU parallelism).
- **Subprocess pool.** Already works today via shell `xargs -P` / GNU parallel. Don't subsume.
- **GPU offload.** Out of scope per `AGENTS.md` Rule 9 (no FFI, no large libraries).
- **Wire protocol for distributed compute.** The `ParmapBackend` trait abstraction in Phase 2 makes this additive; design happens in `dev/plans/rustlab_server.md` when the cluster work begins. See Phase 6 above.
- **Phase-2-time `"threads"` knob to override pool size.** Deferred; rayon's default global pool is the v1 sizing strategy. `nproc()` builtin in Phase 4 lets users *see* the count. Knob added on demand if a real use case hits it.
- **Phase-2-time `"chunks"` knob.** Same — rayon's auto-chunking is the v1 strategy.
- **Static purity analysis of lambdas.** Pure-lambda contract is enforced at runtime (Phase 3), not at parse time. Static analysis is harder and gives the same correctness guarantee.

## Closure conditions

When phases 1–4 are all `shipped`. Move this file to `dev/plans/closed/parmap_parreduce.md` and add a one-line summary to whichever plan is the current "open work" tracker.

## Pending items (post-approval)

Plan is approved as of 2026-05-11. Two items still open but non-blocking:

1. **Forcing-function serialize discipline in Phase 1.** Conditional on Phase 1's bench result: if `Value::Lambda` + `Value::Matrix` round-trip through `rmp-serde` is sub-µs / sub-ms on representative payloads, ship the discipline (worker-boundary serialize even in local mode). If not, skip it for the local backend and rely on the pure-lambda contract. Bench captured in `perf/parmap_serde_overhead.md`.

2. **Real Monte Carlo integration test (optional).** The user has a working MC use case; if they choose to share it, Phase 3 will use it as the headline integration test alongside the invented examples (π estimation, Black-Scholes, bootstrap). Not blocking — Phase 3 can ship with the invented examples and a real case folded in later.
