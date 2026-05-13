# Notebook Future Features

## 5. Template interpolation in markdown cells ✓ DONE

Implemented `${expr}` and `${expr:format_spec}` syntax in markdown cells.
Expressions are evaluated against the shared evaluator during notebook
execution. Format specs use the same `sprintf` formatting as the REPL.
Escape with `\${...}` for literal output. Undefined variables produce
inline `<ERROR: ...>` placeholders.

---

## 6. Cell arrays / string arrays

### Phase 1: String arrays (minimal) ✓ DONE

Implemented `Value::StringArray(Vec<String>)` with `{"a", "b", "c"}` literal
syntax. Added `LBrace`/`RBrace` tokens, `Expr::CellArray` AST node, 1-based
indexing, `length()`/`size()`/`numel()`/`iscell()` support, and Display.

### Phase 1b: Categorical bar chart labels ✓ DONE

`bar({"Jan","Feb","Mar"}, [10,20,30])` produces categorical x-axis labels
in terminal, HTML (Plotly ticktext/tickvals), and SVG/PNG (plotters) output.
Added `x_labels: Option<Vec<String>>` to `SubplotState`.

### Phase 2: Heterogeneous cell arrays
- Add `Value::CellArray(Vec<Value>)` for mixed-type containers
- `{1, "hello", [1 2 3]}` — each element can be any Value
- `cell(n)` constructor for pre-allocated empty cell arrays
- `cellfun()` builtin

### Phase 3: Cell array of strings as axis tick labels ✓ DONE

`plot({"Mon","Tue","Wed","Thu","Fri"}, y)` now produces a categorical
x-axis in all three backends. The builtin mirrors `bar(labels, y)`:
substitutes integer indices 1..=n for x_data and stashes the labels on
`SubplotState.x_labels`. The HTML/Plotly Line trace was updated to emit
the labels as string-valued x when the panel's `x_labels` matches the
series length, matching the existing bar behaviour. ASCII and SVG
backends were already generic on `x_labels` and required no change.

**Scope:** Phase 1, 1b, and 3 cover ~95% of the practical use cases.
