# Development Plan: Lambda / Anonymous Function Support

**Status:** Complete (both phases implemented in commit 6608f52)
**Syntax target:** `@(params) expr` and `@name` function handles

---

## Overview

Add first-class anonymous functions (lambdas) using `@` syntax.
This allows users to write inline math functions, pass functions as arguments,
and compose higher-order operations.

```
f = @(x) x.^2
g = @(x, y) sqrt(x^2 + y^2)
h = @(x) sin(x) .* cos(x)

f(3)           % 9
g(3, 4)        % 5
arrayfun(f, [1, 2, 3])  % [1, 4, 9]
arrayfun(@sin, linspace(0, pi, 5))
```

The implementation is split into two phases.  Phase 1 delivers the core value
type and call dispatch.  Phase 2 adds function handles and higher-order
builtins.  Each phase leaves the project compiling with all existing tests
passing.

---

## Architecture Decisions

### Syntax
Use `@` prefix — familiar to the target audience:
- `@(x) expr` — anonymous function (lambda)
- `@name` — handle to a named function (user-defined or builtin)

Single-expression body only (no `begin...end` block).
Multi-statement bodies require a named `function` definition.

### Scoping
Lambdas capture the environment **lexically** at creation time.
A snapshot of the current env is stored in the value.  Mutations to outer
variables after the lambda is created do not affect it.

```
a = 5
f = @(x) x + a
a = 99
f(1)   % → 6   (captured a=5, not a=99)
```

This is intentional and matches user expectations for a math DSL.

### New `Value` variants
```rust
// In eval/value.rs
Value::Lambda {
    params:       Vec<String>,
    body:         Box<Expr>,
    captured_env: HashMap<String, Value>,
}
Value::FuncHandle(String)   // @sin, @myFn
```

`FuncHandle` stores the name only; dispatch happens at call time through the
normal user_fn / builtin lookup.

### Minimal AST additions
```rust
// In ast.rs — added to Expr enum
Expr::Lambda { params: Vec<String>, body: Box<Expr> }
Expr::FuncHandle(String)
```

### Call dispatch (no AST changes to Expr::Call)
`f(3)` where `f` is a variable already parses as `Expr::Call { name: "f", args: [3] }`.
We add a new check in the evaluator **before** the matrix-indexing fallback:
- If `env["f"]` is `Value::Lambda` → invoke lambda
- If `env["f"]` is `Value::FuncHandle(n)` → redirect to call `n` with same args

This keeps the parser unchanged for the calling side.

### Higher-order builtins
`arrayfun` and `cellfun`-style functions need to call back into the evaluator.
They **cannot** be registered in `builtins.rs` (which only has access to
`Vec<Value>`, not `&mut Evaluator`).  Instead, they are dispatched as special
cases in `eval/mod.rs`'s `eval_expr`, after the normal builtin lookup.

New evaluator-level functions (Phase 2):
- `arrayfun(f, v)` — apply scalar function element-wise over a vector
- `cellfun(f, args...)` — apply function to each column of argument vectors

---

## Phase 1 — Lambda Values and Assignment/Call

**Goal:** `f = @(x) expr` stores a lambda; `f(args)` invokes it.

### Checklist

- [x] **1a. Lexer** — add `Token::At` in `crates/rustlab-script/src/lexer.rs`:
  - Lex `@` as `Token::At`
  - Insert in the character-match arm (near `Token::Bang` or similar)

- [x] **1b. AST** — extend `Expr` in `crates/rustlab-script/src/ast.rs`:
  ```rust
  Lambda { params: Vec<String>, body: Box<Expr> },
  FuncHandle(String),
  ```

- [x] **1c. Parser** — handle `Token::At` in `parse_primary()` in `parser.rs`:
  - Consume `@`
  - If next token is `LParen`:
    - Parse comma-separated `Ident` list as params until `RParen`
    - Parse one expression as body via `parse_expr()`
    - Return `Expr::Lambda { params, body: Box::new(body_expr) }`
  - Else if next token is `Ident(name)`:
    - Consume it; return `Expr::FuncHandle(name)`
  - Else: parse error

- [x] **1d. Value** — add variants to `Value` enum in `eval/value.rs`:
  ```rust
  Lambda {
      params:       Vec<String>,
      body:         Box<Expr>,
      captured_env: HashMap<String, Value>,
  },
  FuncHandle(String),
  ```
  - Add `"lambda"` / `"function_handle"` arms to `type_name()`
  - Add display: `@(x, y) <expr>` for Lambda, `@name` for FuncHandle
  - Add negate/index arms that return a descriptive error

- [x] **1e. Evaluator — eval Expr::Lambda** in `eval/mod.rs`:
  ```rust
  Expr::Lambda { params, body } => {
      Ok(Value::Lambda {
          params: params.clone(),
          body: body.clone(),
          captured_env: self.env.clone(),
      })
  }
  ```

- [x] **1f. Evaluator — eval Expr::FuncHandle** in `eval/mod.rs`:
  ```rust
  Expr::FuncHandle(name) => Ok(Value::FuncHandle(name.clone())),
  ```

- [x] **1g. Evaluator — call dispatch** — in `eval_expr` for `Expr::Call { name, args }`,
  add a check **before** the matrix-indexing fallback (after the builtin lookup):
  ```rust
  // After checking builtins, before treating as matrix index:
  if let Some(val) = self.env.get(name.as_str()).cloned() {
      match val {
          Value::Lambda { params, body, captured_env } => {
              let arg_vals = args.iter()
                  .map(|a| self.eval_expr(a))
                  .collect::<Result<Vec<_>, _>>()?;
              return self.eval_lambda_call(&params, &body, captured_env, arg_vals);
          }
          Value::FuncHandle(target) => {
              // Re-dispatch as Call { name: target, args }
              return self.eval_expr(&Expr::Call { name: target, args: args.clone() });
          }
          _ => {}  // fall through to matrix indexing
      }
  }
  ```

- [x] **1h. Evaluator — `eval_lambda_call` helper** in `eval/mod.rs`:
  ```rust
  fn eval_lambda_call(
      &mut self,
      params: &[String],
      body: &Expr,
      captured_env: HashMap<String, Value>,
      args: Vec<Value>,
  ) -> Result<Value, ScriptError> {
      if args.len() != params.len() {
          return Err(ScriptError::Runtime(format!(
              "lambda expects {} arg(s), got {}", params.len(), args.len()
          )));
      }
      // Save outer env, install captured env + args
      let saved_env = std::mem::replace(&mut self.env, captured_env);
      let saved_in_fn = self.in_function;
      self.in_function = true;
      for (name, val) in params.iter().zip(args) {
          self.env.insert(name.clone(), val);
      }
      let result = self.eval_expr(body);
      // Restore outer env
      self.env = saved_env;
      self.in_function = saved_in_fn;
      result
  }
  ```

- [x] **1i. Unit tests** in `tests.rs`:
  ```
  % Basic lambda
  f = @(x) x^2;
  assert(f(3) == 9)
  assert(f(0) == 0)

  % Multi-arg lambda
  g = @(x, y) sqrt(x^2 + y^2);
  assert(g(3, 4) == 5)

  % Lambda capturing outer variable (lexical scope)
  a = 5;
  h = @(x) x + a;
  a = 99;
  assert(h(1) == 6)   % captured a=5

  % Lambda with vector body expression
  v = [1, 2, 3];
  scale = @(x) x .* 2;
  assert(scale(v) == [2, 4, 6])

  % Lambda calling a builtin
  mysin = @(x) sin(x);
  assert(abs(mysin(pi/2) - 1) < 1e-10)

  % Compose lambdas via calling
  sq = @(x) x^2;
  inc = @(x) x + 1;
  assert(sq(inc(2)) == 9)
  ```

---

## Phase 2 — Function Handles and Higher-Order Builtins

**Goal:** `@name` handles work; `arrayfun` applies a function element-wise.

### Checklist

- [x] **2a. `@name` handle calling** — verify Phase 1 dispatch covers this:
  `h = @sin; h(pi/2)` → FuncHandle("sin") → redispatch to Call{sin, [pi/2]} ✓
  Also test with user-defined function names.

- [x] **2b. `arrayfun(f, v)` builtin** — implemented as an evaluator-level
  special case in `eval_expr` (not in builtins.rs) since it needs `&mut self`:
  ```rust
  // In eval_expr, Expr::Call { name, args } — check name == "arrayfun"
  if name == "arrayfun" && args.len() == 2 {
      let func_val = self.eval_expr(&args[0])?;
      let vec_val  = self.eval_expr(&args[1])?;
      return self.eval_arrayfun(func_val, vec_val);
  }
  ```
  `eval_arrayfun` iterates each element of the input vector/matrix,
  calls the lambda or function handle on it, and assembles the result.
  - Input is `Value::Vector` or `Value::Matrix`; scalar inputs return scalar
  - Result type matches output of `f` on each element
  - Error if `f` returns a non-scalar for a vector input (element-wise only)

- [x] **2c. `feval(name, args...)` builtin** — call a function by string name:
  ```
  feval("sin", pi/2)   % same as sin(pi/2)
  feval("myFunc", 3)
  ```
  Implemented as evaluator-level special case.  Looks up `name` in
  user_fns first, then builtins, then env (for lambdas stored by string var).

- [x] **2d. Unit tests** in `tests.rs`:
  ```
  % Function handle to builtin
  h = @sin;
  assert(abs(h(pi/2) - 1) < 1e-10)

  % Function handle to user function
  function y = double_it(x)
    y = x * 2;
  end
  d = @double_it;
  assert(d(7) == 14)

  % arrayfun with lambda
  f = @(x) x^2;
  result = arrayfun(f, [1, 2, 3, 4]);
  assert(result == [1, 4, 9, 16])

  % arrayfun with function handle
  result2 = arrayfun(@sqrt, [1, 4, 9, 16]);
  assert(result2 == [1, 2, 3, 4])

  % feval
  assert(feval("sin", pi/2) == 1)

  % Passing lambdas to custom functions
  function y = apply_twice(f, x)
    y = f(f(x));
  end
  sq = @(x) x^2;
  assert(apply_twice(sq, 2) == 16)  % (2^2)^2
  ```

---

## Key Files Modified

| File | Phase | Change |
|---|---|---|
| `crates/rustlab-script/src/lexer.rs` | 1 | Add `Token::At` |
| `crates/rustlab-script/src/ast.rs` | 1 | Add `Expr::Lambda`, `Expr::FuncHandle` |
| `crates/rustlab-script/src/parser.rs` | 1 | Parse `@(p) expr` and `@name` in `parse_primary` |
| `crates/rustlab-script/src/eval/value.rs` | 1 | Add `Value::Lambda`, `Value::FuncHandle` |
| `crates/rustlab-script/src/eval/mod.rs` | 1, 2 | Eval new exprs; call dispatch; `eval_lambda_call`; `arrayfun`/`feval` |
| `crates/rustlab-script/src/tests.rs` | 1, 2 | New test cases |
| `docs/quickref.md` | 2 | Document `@` syntax |
| `AGENTS.md` | 2 | Note lambda support |

---

## Design Rules for the Implementing Agent

1. **Single-expression body only.** Lambdas are `@(x) expr`, not
   `@(x) { stmt; stmt }`.  If multi-statement bodies are needed, users define
   a named function.

2. **Lexical capture is a full clone.** `self.env.clone()` on creation.
   This is simple and correct; optimisation (e.g., only capturing free variables)
   is a future concern.

3. **`eval_lambda_call` must not suppress output differently than a call inside
   `eval_user_fn`.** Set `in_function = true` for the duration.

4. **`arrayfun` and `feval` live in `eval/mod.rs`, not `builtins.rs`.** They
   need `&mut self` to call back into the evaluator.  Mark them with a comment
   explaining why they are not in the builtin registry.

5. **All existing tests must pass** after each phase.  Run `cargo test` before
   wrapping up a session.

6. **No new crate dependencies.** Everything is implemented with existing types.

7. **Follow commit style** — no `Co-Authored-By`, no force push.

---

## Implementation Order

```
Session 1 → Phase 1 (lexer → AST → parser → value → evaluator → tests)
Session 2 → Phase 2 (arrayfun, feval, docs, AGENTS.md)
```
