# rustlab v0.3 — Language additions

Four language features landed in v0.3 to close gaps that downstream
projects (notably `rustlab_llm`) hit when porting matlab/octave code or
writing transformer-style numerics. They are independent — each one is
useful on its own — but this notebook walks through them as a tour.

| Feature | Form |
|---|---|
| Multi-output user functions | `function [a, b] = name(x)` |
| Short-circuit logical ops | `&&`, `\|\|` (scalar truthiness) |
| Linear-index matrix gather | `M(k)`, `M(I)`, `M(find(...))` |
| `layernorm` matrix overload | `layernorm(M[, dim[, eps]])` |

Three of the four are matlab-compat improvements; `layernorm(M)` is a
rustlab-native ML feature with the per-row default that PyTorch / JAX
use.

## 1. Multi-output user functions — `function [a, b, ...] = name(x)`

Bracket the output list to declare more than one return. Matlab
convention. The caller destructures with `[p, q] = ...`; a bare
`v = ...` picks just the first declared output.

```rustlab
function [m, idx] = max_with_pos(v)
  m = max(v)
  idx = argmax(v)
end

[best, where] = max_with_pos([3, 1, 4, 1, 5, 9, 2, 6]);
print(best)        % → 9
print(where)       % → 6  (1-based position of max)
```

Single-output use of the same multi-output function only picks `m`:

```rustlab
just_max = max_with_pos([10, 20, 5]);
print(just_max)    % → 20
```

Errors are loud:

- Asking for more outputs than declared (`[a, b, c] = pair(x)` where
  `pair` declares 2) → arity error.
- Function declares an output variable that the body never assigns →
  missing-assignment error (only enforced for multi-output declarations
  to preserve back-compat).
- `function [] = name(x)` is a parse error — write
  `function name(x)` instead for the no-return form.

## 2. Short-circuit `&&` and `||` with scalar truthiness

Logical `&&` / `||` short-circuit and accept any non-zero scalar as
truthy (matlab convention). The right-hand side is only evaluated when
the left isn't decisive — useful for guarding undefined operations.

```rustlab
% Guard a divide-by-zero. RHS is unreachable when x == 0.
x = 0;
safe = (x != 0) && (1.0 / x > 0);
print(safe)        % → false  (no division attempted)

% Or-form short-circuit.
n = 5;
in_set = (n == 1) || (n == 5) || (n == 99);
print(in_set)      % → true  (stops at the second clause)
```

Scalar operands work directly — no need to wrap in a comparison:

```rustlab
print(1 && 2)      % → true   (both non-zero)
print(0 || 3)      % → true   (rhs decides)
print(0 && 1)      % → false  (lhs short-circuits)
```

Matrix or vector operands error — use `any(...)` or `all(...)` to
collapse them first.

## 3. Column-major linear matrix indexing — `M(k)`, `M(I)`, `M(find(...))`

Single-arg matrix indexing is column-major linear, matching matlab.
For row extraction, use the explicit two-arg form `M(i, :)`.

```rustlab
M = [10, 20; 30, 40; 50, 60];
print(M(:))             % → [10, 30, 50, 20, 40, 60]  (col-major flatten)
print(M(2))             % → 30   (second linear element)
print(M([1, 4, 6]))     % → [10, 20, 60]   (vector of picks)
print(M(2, :))          % → [30, 40]   (row 2 — explicit form)
```

This lets `find` round-trip naturally — `find(...)` returns 1-based
linear indices in storage order, and `M(find(...))` picks them back:

```rustlab
ix = find([0, 1, 0, 1, 0, 1]);   % → [2, 4, 6]
print(M(ix))                       % → [30, 20, 60]
```

> **Breaking change from v0.2**: `M(scalar)` on a matrix used to return
> the n-th row. It now returns a single linear element. Migrate any
> code that relied on the old behavior to `M(scalar, :)`.

## 4. `layernorm(M)` matrix overload — per-row by default

Layer normalisation now accepts matrices as well as vectors. The
default normalises each **row** independently (dim=2), matching
PyTorch / JAX where rows are samples and columns are features.
Override with `dim=1` for per-column.

```rustlab
S = [1, 2, 3, 4, 5; 100, 200, 300, 400, 500];
Sn = layernorm(S);
print(mean(Sn, 2))        % per-row means → ~0 in each row

% layernorm uses population variance (divide by N), but std() uses
% the sample form (N-1). Verify the per-row population variance
% directly instead.
print(mean(Sn .* Sn, 2))  % → ~1 in each row
```

Override the default axis:

```rustlab
Sc = layernorm(S, 1);     % per-column normalisation
print(mean(Sc))           % per-column means (default dim=1) → ~0
```

Custom epsilon (third arg in the matrix form):

```rustlab
Se = layernorm(S, 2, 1e-8);
print(mean(Se(1, :)))     % row 1 still has zero mean
```

> **Why per-row by default?** `layernorm` is an ML primitive; per-row
> matches the transformer convention where each row is a token / sample.
> This intentionally diverges from `sum`/`mean`/`std`, which default to
> `dim=1` (per-column, octave convention).

## Verifying everything ran

```rustlab
print(1)   % sentinel — if you see this in the output, all four
           % feature demos above completed without errors
```
