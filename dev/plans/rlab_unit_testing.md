# `.rlab` Unit-Testing Framework

A user-facing testing framework for `.rlab` scripts: assertion builtins, a
`rustlab test` runner with discovery, named test cases, setup/teardown with
fixtures, per-test isolation, and structured (human / TAP / JSON) output.

## Status & handoff (read this first)

- **State:** plan approved, **no implementation written**. The working tree is
  clean except for this file.
- **Branch:** `feature/rlab-unit-testing` (created off `main`). Do all work here;
  do not push to `main` directly (workflow rule 7).
- **Approved choices** (from the user, do not re-litigate):
  1. Fixtures: `setup()` *returns* a value; the runner passes it to
     `test_foo(fix)`. Tests take 0 or 1 param.
  2. Discovery globs: both `*_test.rlab` **and** `test_*.rlab`.
  3. `assert_error` / `try-catch` is **deferred** (Phase 6, do not build yet).
- **Approvals still required before committing:** `git commit` and `git push`
  need explicit user sign-off (`git add` is fine). Never merge the PR. No MATLAB
  references in any shipped artefact (code/docs/help/examples/tests).
- **What was abandoned mid-edit and reverted:** an earlier pass created
  `crates/rustlab-script/src/eval/assertions.rs` and registered the module in
  `eval/mod.rs`, then backed both out at the user's request. Phase 1 below is the
  intended shape of that module — start fresh from it.
- **Where to start:** Phase 1 (assertions) → Phase 2 (runner API) → Phase 3
  (`rustlab test`). All concrete file/line anchors are in the appendix at the
  bottom; they were verified against the tree at plan time but re-confirm before
  editing (line numbers drift).
- **Validation:** `make test` (workspace + features) plus a manual
  `cargo run -p rustlab-cli -- test examples/testing/` once Phase 3 lands.

## Goals

Let users of rustlab write and run organized unit tests for their own DSP /
matrix scripts, following standard xUnit/pytest-style patterns — assertions,
auto-discovered test cases, fixtures, and a runner that reports pass/fail and
returns a CI-friendly exit code.

## Key design decisions (and why)

- **The Rust runner catches failures, not the language.** The `.rlab` language
  has no `try/catch`, and builtins (`fn(Vec<Value>) -> Result<Value, ScriptError>`)
  cannot invoke a lambda and catch its error. So an assertion simply returns
  `Err(ScriptError::runtime(..))`, and the *runner* (in `rustlab-cli`) catches
  that per test, records it, and continues to the next test. This keeps the
  language untouched and matches how the evaluator already surfaces errors.
- **Tests are plain functions, discovered by name.** Reuses existing
  `FunctionDef`/closure machinery (KISS/DRY) — no new parser syntax. A test is a
  function whose name begins with `test_`. This is the pytest convention.
- **Fixtures are passed, not shared via globals.** rustlab functions have
  isolated scope (like MATLAB) — `setup()` cannot leak locals into a test via
  the base workspace. So `setup()` *returns* a fixture value (often a
  `struct(...)`), and the runner passes it as the test's argument. A test may
  take 0 or 1 parameter.
- **Per-test isolation via `Evaluator::deep_clone()`.** The runner loads the
  file once into a base evaluator (registering all defs + module constants),
  then `deep_clone()`s it before each test so state never leaks between tests.
- **Lives in the main `rustlab` binary.** The runner reuses the interpreter
  already linked into the CLI, so the marginal binary-size cost is just the
  discovery/run/report loop — negligible. (Unlike notebook subcommands, a test
  runner for `.rlab` scripts is core scripting functionality.)
- **`assert_error`/`assert_throws` is deferred.** Asserting that code *fails*
  requires catching an error mid-test, which needs either a `try/catch` language
  construct or an evaluator-aware assertion. Scoped as an optional later phase.

## Conventions (defaults — open to change)

- **Test files:** `*_test.rlab` or `test_*.rlab` (recursive discovery under the
  given path; hidden dirs skipped).
- **Test functions:** name begins with `test_`, takes 0 or 1 param.
- **Hooks:** `setup()` → returns a fixture (runs before each test);
  `teardown()` / `teardown(fixture)` (runs after each test, even on failure).
  `setup_all` / `teardown_all` deferred (see Phase 6).
- **Display name:** the function name (e.g. `test_lowpass_gain`).

## Assertion builtins (Phase 1)

All return `Value::None` on success and `Err(ScriptError::runtime(msg))` with a
descriptive message on failure (the evaluator attaches the source line).

- `assert(cond)` / `assert(cond, msg)` — `cond` must be truthy: `Bool(true)`,
  a nonzero scalar/complex, or an array with all elements nonzero.
- `assert_true(x)` / `assert_false(x)`
- `assert_equal(a, b)` / `assert_equal(a, b, msg)` — exact structural equality
  (shape + values; works for bool/str/scalar/complex/vector/matrix).
- `assert_near(a, b)` / `assert_near(a, b, tol)` — element-wise tolerance
  (default `1e-9`, combined abs/rel). On failure reports the first offending
  index and the magnitude of the difference.
- `fail(msg)` — unconditional failure.
- Plus two general-purpose, non-asserting helpers reused by the asserts:
  `isequal(a, b)` → `Bool`, and `allclose(a, b[, tol])` → `Bool`. (Useful in
  ordinary scripts too; assertions are thin wrappers over these.)

New module `crates/rustlab-script/src/eval/assertions.rs`; register the builtins
in `BuiltinRegistry::with_defaults()`. Comparison core (`values_equal`,
`values_near` returning `Result<(), String>` diff messages) lives there and is
shared by both the `assert_*` and `isequal`/`allclose` builtins.

## rustlab-script runner API (Phase 2)

Small public additions on `Evaluator` (the call-by-name path is currently
private):

- `pub fn eval_source(&mut self, src: &str) -> Result<(), ScriptError>` —
  lex + parse + run a source string into `self` (so the runner can load a file
  and keep the populated evaluator).
- `pub fn call_named(&mut self, name: &str, args: Vec<Value>) -> Result<Value, ScriptError>`
  — thin wrapper over the existing `eval_feval`.
- `pub fn user_fn_arity(&self, name: &str) -> Option<usize>`.

(`user_fn_names()`, `is_user_fn_defined()`, `deep_clone()`, `get()` already
exist and are reused.)

## `rustlab test` subcommand (Phase 3)

New `crates/rustlab-cli/src/commands/test.rs`; add `Test(TestArgs)` to the
`Commands` enum in `cli.rs` and a dispatch arm.

```
rustlab test [PATH]...        # files and/or dirs; default: ./ (or ./tests)
  -k, --filter <SUBSTR>       # only run tests whose name contains SUBSTR
  --format <human|tap|json>   # default: human
  --fail-fast                 # stop after the first failing test
  --no-color                  # force-disable color (auto-off when not a tty)
```

Per-file execution model:

1. Read file; `eval_source` into a base `Evaluator`. A lex/parse/top-level error
   is reported as a file-level error (counts as a failure).
2. Discover via `user_fn_names()`: tests = names starting `test_` (sorted for
   determinism); note whether `setup`/`teardown` exist and their arity.
3. For each test:
   - `ev = base.deep_clone()` (isolation).
   - `fixture = setup ? ev.call_named("setup", []) : None` (setup error ⇒ test
     errored).
   - `args = test arity == 1 ? [fixture] : []`.
   - time and run `ev.call_named(test, args)`, catching `ScriptError`.
   - run `teardown` (with fixture if it takes one) regardless; a teardown error
     marks the test errored.
   - record `{file, name, status: pass|fail|error, duration, message}`.
4. Aggregate across files; exit `0` if all passed, `1` otherwise.

Reporting:

- **human** (default): per-file header, `✓ name (1.2ms)` / `✗ name — message`,
  colorized when stdout is a tty; summary line `N passed, M failed in X.XXs`.
- **tap**: TAP version 13.
- **json**: `{ summary, tests: [...] }` for machine consumption.

Reuse the existing CLI error-formatting used by `run`/`repl` for parse errors.

## Structured output formats (Phase 4)

Implement the `tap` and `json` formatters behind `--format`. (Split from Phase 3
so the human runner can be reviewed first.)

## Docs, help, examples, tests (Phase 5)

- **REPL help** (`commands/repl.rs` `HELP` table): entries for `assert`,
  `assert_true`, `assert_false`, `assert_equal`, `assert_near`, `fail`,
  `isequal`, `allclose`. *(workflow rule: update REPL help)*
- **docs/functions.md** + **docs/quickref.md**: new "Testing" section.
- **AGENTS.md**: document the testing framework, conventions, and `rustlab test`.
  *(workflow rule: update AGENTS.md)*
- **examples/testing/**: a sample `signal_test.rlab` exercising every assertion
  type plus `setup`/`teardown` with a fixture. (Running it directly via
  `rustlab run` just defines functions — harmless for the examples CI sweep.)
- **README.md**: short "Testing your scripts" subsection.
- **Rust tests** *(workflow rule: test with features)*:
  - unit tests for each assertion builtin + `isequal`/`allclose` in
    `rustlab-script` (`tests.rs`).
  - a `rustlab-cli` integration test that runs a fixture `*_test.rlab`
    (mix of pass/fail) and asserts the summary counts and exit code.

## Optional / stretch (Phase 6)

- `assert_error(@() expr)` / `assert_throws` — assert that a thunk fails. Needs
  either a `try/catch` language construct (generally useful, larger scope) or an
  evaluator-aware assertion path. Decide separately.
- `setup_all` / `teardown_all` file-level hooks (only useful for external side
  effects given function-scope isolation).
- `--isolate` toggle, test timeouts, `#[ignore]`-style skip markers.

## Process notes (per workflow rules)

- New feature branch; no direct pushes to `main`. *(workflow rule 7)*
- `git add` only when staging; no commit/push without explicit approval.
- No MATLAB references in any shipped artefact (code/docs/help/examples/tests).
- Validate with `make test` (workspace + features) and a manual `rustlab test`
  run on the example file.

## Suggested order

Phase 1 (assertions + their Rust tests) → Phase 2 (runner API) → Phase 3
(`rustlab test`, human output) → Phase 4 (TAP/JSON) → Phase 5 (docs/help/
examples/integration test). Phase 6 only if desired.

## Appendix: codebase anchors

Verified against the tree at plan time. **Re-confirm line numbers before
editing — they drift.** Paths are repo-relative.

### Builtins — where assertions plug in (`crates/rustlab-script/src/eval/builtins.rs`)
- `pub type BuiltinFn = fn(Vec<Value>) -> Result<Value, ScriptError>;` — line ~35.
  Nargout variant ~40 (assertions don't need it).
- `BuiltinRegistry::new()` ~54, `with_defaults()` ~60. The long run of
  `r.register("name", builtin_fn);` calls ends at ~400 — **add the assertion
  registrations there.**
- Dispatch `call_with_nargout` ~416; `check_args` ~450, `check_args_range` ~462
  (both private — Phase 1 re-implements a small `arity` helper in its own module).
- Reference builtin to copy the shape from: `builtin_fir_lowpass` ~486.

### Value & numeric types
- `enum Value` — `crates/rustlab-script/src/eval/value.rs:117`. Relevant variants:
  `Scalar`, `Complex`, `Vector(CVector)`, `Matrix(CMatrix)`, `Tensor3(CTensor3)`,
  `Bool` (126), `Str` (127), `StringArray`, `None` (137).
- Helpers: `type_name()` :235, `to_scalar()` :1752, `to_usize()` :1761,
  `to_str()` :1770, `deep_clone()` :2200.
- Type aliases — `crates/rustlab-core/src/types.rs:5-11`: `C64 = Complex<f64>`,
  `CVector = Array1<C64>`, `CMatrix = Array2<C64>`, `CTensor3 = Array3<C64>`.
  ndarray `.iter()` yields logical (row-major) order — the order the Phase 1
  `extract_numeric` flattens into and the index labels assume.

### Errors (`crates/rustlab-script/src/error.rs`)
- `enum ScriptError` :3. Constructors: `runtime(String)` :63, `type_err(String)`
  :67, `arg_count` :79, `arg_count_range` :88. `with_line()` :100 — the evaluator
  stamps the source line onto a `Runtime { line: 0, .. }`, so assertion builtins
  can leave line 0 and still report the right location.

### Evaluator — runner API (`crates/rustlab-script/src/eval/mod.rs`)
- `struct Evaluator` :38 (`env`, `builtins`, `user_fns`, …).
- `pub fn deep_clone()` :77 — **public; the per-test isolation primitive.**
- `pub fn new()` :105, `pub fn get()` :169, `pub fn is_user_fn_defined()` :189,
  `pub fn user_fn_names()` :217.
- `fn eval_feval(name, args)` :2178 — **private** call-by-name; Phase 2 wraps it
  as `pub fn call_named`. `eval_user_fn` :2285, `eval_user_fn_nargout` :2296
  (the latter has arity + return-shaping logic; `user_fn_arity` reads `UserFn.params`).
- `FunctionDef` registration into `user_fns` :309-338.
- Module decls at top of file :1-10 — **add `pub mod assertions;` here.**

### Crate entry points (`crates/rustlab-script/src/lib.rs`)
- `pub use error::ScriptError` :45, `pub use eval::Evaluator` :47,
  `pub use eval::Value` :48.
- `pub mod lexer` :38, `pub mod parser` :39 — public, so Phase 2's `eval_source`
  can lex+parse+`run_script` in one call (or the runner could parse directly).
- `pub fn run(source)` :85 builds its own evaluator — **not** reusable for the
  runner (it discards the populated evaluator); hence the new `eval_source`.

### CLI subcommand (`crates/rustlab-cli/src`)
- `enum Commands` — `cli.rs:17` (clap `#[derive(Subcommand)]`); dispatch
  `pub fn execute()` :41 (match arms :43-51). **Add `Test(TestArgs)` + a match arm.**
- Module list — `commands/mod.rs` (`pub mod cache; … pub mod window;`). **Add
  `pub mod test;`.**
- Existing script-running command for reference: `commands/run.rs` — `execute()`
  :35, reads file :51, calls `rustlab_script::Evaluator::new()` :68 then
  `super::repl::run_script_source(&source, &mut ev)` :69.
- `run_script_source` is defined in `commands/repl.rs` (pub fn) — reuse its
  error formatting for parse/top-level errors.

### Help & docs
- `commands/repl.rs`: `pub struct HelpEntry` :14, `pub const HELP: &[HelpEntry]`
  :20 (source of truth), `print_help_list()` :1306, `print_help_detail()` :1346.
  **Add one `HelpEntry` per new builtin.**
- `docs/functions.md`, `docs/quickref.md` — hand-maintained, mirror HELP. Add a
  "Testing" section to each.
- `AGENTS.md` — authoritative agent guide; document the framework + `rustlab test`.

### Tests & CI
- Rust unit-test pattern: `crates/rustlab-script/src/tests.rs` — `eval_str`
  helper near the top builds an `Evaluator` from source; `close(a,b)` float
  helper ~:541. Put Phase 1 assertion tests here.
- `Makefile` test target ~:19: `cargo test --workspace --features viewer` then
  `cargo test -p rustlab-notebook --features mermaid`.
- Examples live under `examples/<toolbox>/`; CI sweeps run each `.rlab`. A test
  file run directly via `rustlab run` only defines functions (no auto-run), so a
  sample `examples/testing/*_test.rlab` is harmless to the sweep.
