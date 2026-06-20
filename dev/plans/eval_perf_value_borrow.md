# Script Evaluator Performance — Stop Cloning `Value` on Hot Paths

A staged refactor of the `rustlab-script` evaluator to eliminate the full
`Value` clones it pays on every variable read, index, lambda creation, and
per-element higher-order call. Estimated **3–5× on arrayfun-heavy scripts**.

## Status & handoff (read this first)

- **State:** plan approved by the user (2026-06-13). **No implementation
  written yet** — the working tree is clean except for this file.
- **Branch:** `feature/eval-perf-borrow` (created off `main`). Do all work
  here; do not push to `main` directly (workflow Rule 7). One PR.
- **Approvals still required before committing:** `git commit` and `git push`
  need explicit user sign-off (`git add` is fine; Rule 3). Never merge the PR
  (the landing call is the user's).
- **Origin:** deferred from the 2026-06-11 review (PRs #19/#20). The original
  finding lives in the `deferred-value-borrow-refactor` auto-memory; update it
  after this lands (done items + residuals).
- **Design status:** pressure-tested against the actual code by a validation
  pass. All four phases confirmed feasible; the gotchas that pass surfaced are
  folded into the steps below — **do not re-litigate them.**

All file references are in `crates/rustlab-script/src/` unless noted.

---

## Problem

The evaluator clones full `Value`s on every hot path:

| Site | What it clones | Frequency |
|---|---|---|
| `mod.rs:1704` (Call-as-indexing) | the whole container (Vector/Matrix/Tensor3) | every `v(i)`, `M(i,j)` read |
| `mod.rs:1572` (`Expr::Var`) | the whole value | every variable reference |
| `mod.rs:1917` (`Expr::Lambda`) | the **entire** environment | every `@(x) …` creation |
| `mod.rs:2012 / 2195 / parmap.rs:179` | the whole lambda (body AST + captured env) | **per element** in arrayfun/rk4/parmap |
| `mod.rs:1781` (Call → user fn) | the whole `UserFn` AST | every user-function call |

The arrayfun case is the worst: a lambda that captures a fat env, deep-cloned
once per element. That is the 3–5× target.

---

## Phase 0 — Benchmarks first (required by the deferral note)

The deferral note (Rule 1) requires benchmarks before code.

1. Add `perf/bench_lambda.rlab` following existing `bench_*.rlab` conventions
   (printed section headers, deterministic, no plotting):
   - arrayfun with a lambda over n=20k **with several large vectors in scope**
     (stresses env snapshot + per-element clone — the headline case)
   - bare lambda call in a `for` loop, n=20k (measures the residual
     direct-call clone that this refactor does *not* fully fix)
   - read-indexing loop `s = s + v(i)` over n=100k (container clone)
   - user-function call loop, n=20k (per-call AST clone)
   - parmap vs arrayfun on the same heavy lambda (correctness cross-check, in
     the spirit of `examples/language/bench_parmap.rlab`)
2. Baseline on the unmodified branch point: `cargo build --release`, then
   median-of-3 wall times for `bench_lambda.rlab` + `bench_interpreter.rlab`.
   Record in `perf/value_borrow_2026_06.md` with the usual timestamp/platform
   header. Re-run and append before/after numbers after each phase.

---

## Phase 1 — Read-only indexing borrows  *(low risk)*

**`eval/value.rs:540–961`** — convert `index`, `index_1d`, `index_2d`,
`index_3d` from `self` to `&self`:

- Full-slice arms (`v(:)` ~563, Str/StringArray `All` ~718/747, Tensor3
  All/All/All ~900) become `.clone()` — cost-identical to today (the call site
  already cloned the whole container).
- Tuple scalar pick (~707) becomes `items[i].clone()` — strictly better.
- `index_2d` Vector fallback (~831/834) calls `self.index_1d(row_idx)` directly
  instead of rebuilding a `Value`.
- `resolve_index_dim` (509) already takes `&Value`.
- Mutation paths (`IndexAssign`, `exec_index_delete`, `tensor3_index_assign`)
  do **not** use these helpers — leave untouched. Existing `tests.rs` callers
  (747, 756) compile unchanged via auto-ref.

**`eval/mod.rs:1693–1780`** (Call-as-indexing arm) — drop the wholesale
`container.clone()`:

- Under a short immutable borrow, copy out a small local
  `enum Dims { T3(m,n,p), Mat(r,c), Linear(n), VecAsMat(r,c) }`. It must carry
  the **type tag**, not just sizes — the 2-arg branch (~1736) decides `end`
  binding on Matrix/SparseMatrix-ness, not just `nrows`.
- **E0502 trap:** drop that borrow *before* the first
  `self.env.insert("end", …)`. Evaluate the index expressions (they need
  `&mut self`), then re-borrow via `self.env.get(name).ok_or_else(...)` (not
  `self.env[..]`) and call `container.index(idx_vals)` by ref.

**`eval/mod.rs:1895–1912`** (`Expr::Index` arm) — special-case `Expr::Var`
containers to borrow; arbitrary sub-expressions keep `eval_expr` (they are
temporaries anyway).

**`eval/mod.rs:1928–1949`** (`Expr::Field` arm) — special-case
`object = Expr::Var`: borrow the struct, clone only the requested field
(StateSpace: clone only the one requested matrix). Preserve
`ScriptError::undefined` for a missing var. The write path is
`StmtKind::FieldAssign` (mod.rs:411–428), separate — unaffected.

---

## Phase 2 — Lambda capture-set analysis  *(medium risk)*

Capture only the env entries the lambda body actually references.

- Build the free-identifier walker as a variant of the existing
  `purity::visit_expr_for_var_refs` (`purity.rs:468–530` — already collects
  Var + Call + FuncHandle names and excludes nested-lambda params). Add:
  (a) collect string-literal first args of `feval` calls;
  (b) return a `dynamic_feval_seen` flag when a `feval` first arg is **not** a
  string literal.
- **`eval/mod.rs:1914–1918`** — capture = (free idents of body − params) ∩
  `env` keys. If `dynamic_feval_seen`, fall back to a full `env.clone()`
  (soundness).
- **Do not hardcode a constant list.** `i, j, pi, e, Inf, NaN, true, false`
  are ordinary env entries inserted at `Evaluator::new` (mod.rs:131–147); the
  intersection rule captures them automatically. (The six-name list at
  mod.rs:2478 omits `true`/`false` — do not copy it.)
- **FuncHandle indirection:** after computing the set, for any captured value
  that is `Value::FuncHandle(target)`, transitively capture `target` from env
  too — a stored handle resolves its name against the captured env at call time
  (mod.rs:1802 / 2289).
- `end` needs no handling: absent from env at creation; the indexing site
  inserts it into the live env during the call.
- `user_fns` is a separate map that survives the env swap — never captured.

**Tests before changing capture** (semantics must stay identical):
- Pins that must keep passing: `lambda_capture_is_snapshot` (tests.rs:8513),
  `lambda_returning_lambda` (tests.rs:8580).
- New tests to add first: `feval("g", x)` of a captured env-lambda from inside
  another lambda; a stored FuncHandle-to-env-lambda called inside a lambda; a
  lambda using `pi` / `true`.

---

## Phase 3 — Stop per-call deep clones  *(low–medium risk)*

**3a — `call_callable` (mod.rs:2272) takes `&Value`.** The Lambda arm
destructures by ref and clones only `captured_env` (small after Phase 2);
`eval_lambda_call` already takes `&[String]` / `&Expr` / owned env — no change.
Update call sites mod.rs:2012 (arrayfun), mod.rs:2195 (rk4), parmap.rs:179 (the
rayon closure stops cloning; the `ParmapBackend::run` trait signature can stay).
Parmap workers are per-element and stateless between elements — no behavior
change.

**3b — `user_fns: HashMap<String, Arc<UserFn>>`.** Arc is mandatory:
`Evaluator` is `Clone` and sent to rayon workers, and a borrowed `&UserFn` from
the map into `eval_user_fn_nargout(&mut self, …)` is an E0502 conflict — clone
the Arc and pass it owned. Touch points: mod.rs:363 (insert → `Arc::new`), 637
(MultiAssign dispatch), 1781 (Call arm), 2254 (`eval_feval`), 2360/2371 (fn
signatures), cache paths 2669–2673, 2908/2921, 2957, 3026. `deep_clone`
(mod.rs:95–126) survives unchanged. **Bonus:** parmap's per-element
`template.clone()` stops deep-cloning every user-fn AST.

**3b-adjacent free win:** `eval_user_fn_nargout` return harvesting
(mod.rs:2502–2506) — `remove()` instead of `get().cloned()` from the
about-to-drop function env.

---

## Phase 4 — Binop borrowing  *(measure first, then decide)*

After Phase 3, re-run benchmarks. Implement only if vector-loop arithmetic
(`bench_interpreter` expression chains / `s = s + v(i)`) still shows
operand-clone dominance.

- Convert the `Value::binop` body (value.rs:990–1653) to
  `binop_ref(op, &Value, &Value)`; keep `binop(self-args)` as a thin forwarding
  wrapper (tests call it directly). **Verified: no arm exploits ownership
  today** (every arm allocates fresh output; there is no `mapv_into`), so this
  is mechanical. Chores: `promote_to_complex` takes `&Value`; the sparse
  fallback arms (1584–1644) need let-bound dense temporaries; keep the Tensor3
  scalar-broadcast `scalar_on_left` logic (1370–1414) byte-identical (operand
  order matters for Sub/Div/Pow).
- Eval arm (mod.rs:1585–1605): when an operand is `Expr::Var`, skip the env
  clone — a `contains_key` pre-check on a Var lhs preserves undefined-error
  ordering before rhs evaluation; the both-Var case borrows both refs at once.
  `&&` / `||` are already diverted to the short-circuit path earlier.

---

## Known residual limits (documented, out of scope)

- Direct lambda call `f(3)` in a scalar loop still clones the Lambda body AST
  per call (mod.rs:1787, eval_feval:2257) — fixing needs `Arc<Expr>` inside
  `Value::Lambda` (larger change). Phase 2 shrinks the env part; arrayfun-style
  loops are covered by 3a.
- `Expr::Var` reads feeding **builtin calls** (e.g. `sum(v)`) still clone —
  builtins take owned `Vec<Value>`. This is the structural ceiling of the
  refactor; benchmarks should not expect builtin-call sites to improve.

---

## Tests & docs (Rules 2 / 4 / 5 / 6)

- New tests in `src/tests.rs` (`lambda_tests` / `evaluator_edge_tests` /
  `index_assign_tests` areas): the capture-set cases from Phase 2; `v(:)` /
  tuple / Str indexing still correct under `&self`; struct field read on an
  undefined var errors; arrayfun / rk4 / parmap outputs unchanged (the
  `parmap_phase3` determinism tests are the 3b regression net).
- `cargo test --workspace` green after each phase; also run
  `examples/language/bench_parmap.rlab` and `lambda_pipeline.rlab` by hand.
- **No new builtins** → quickref (Rule 5) and REPL help (Rule 6) need no
  changes. **AGENTS.md** (Rule 4): no new feature rows; the dynamic-feval
  fallback keeps observable semantics identical, so nothing to document there
  (re-confirm during implementation).

---

## Verification

1. `cargo test --workspace` after each phase.
2. `cargo build --release`; median-of-3 timings of `perf/bench_lambda.rlab`,
   `perf/bench_interpreter.rlab`, `perf/bench_builtins.rlab` before/after each
   phase, recorded in `perf/value_borrow_2026_06.md`.
3. `examples/language/bench_parmap.rlab` — assert seq/par max-abs error is 0,
   as the script already does.
4. **Success target:** ≥3× on the arrayfun-with-fat-env benchmark; no
   regression >2% on any existing bench.
