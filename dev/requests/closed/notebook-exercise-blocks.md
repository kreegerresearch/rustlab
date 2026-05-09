# Feature Request: Exercise blocks with hidden solutions

> **STATUS: RESOLVED.** `<!-- exercise -->` and `<!-- solution -->`
> directives ship; exercises are auto-numbered, solutions render as
> collapsed `<details>` widgets. See `docs/notebooks.md` § "Exercises
> and solutions" and `crates/rustlab-notebook/src/parse.rs` for the
> parser branches.

## Problem

The quantum_lab lessons have exercises in their `lesson.md` files but these aren't integrated into the notebooks. A tutorial notebook should let students attempt a problem before revealing the answer. Currently there is no way to present a question with a hidden solution inline.

## Proposed directives

`<!-- exercise -->` marks a question block. `<!-- solution -->` marks the corresponding answer, collapsed by default.

### Usage

```markdown
<!-- exercise -->
What detuning $\Delta$ reduces the maximum excited-state population to exactly 50%?

<!-- solution -->
From $P_{e,\max} = \Omega^2/(\Omega^2 + \Delta^2) = 0.5$, we get $\Delta = \Omega$.

```rustlab
Delta = 1.0
Omega = 1.0
P_max = Omega^2 / (Omega^2 + Delta^2)
```

The simulation confirms $P_{e,\max} = ${P_max:%.1f}$.
```

### Rendered output

The exercise text is always visible, styled with a distinct border/background (e.g., a numbered "Exercise N" header). The solution is collapsed behind a "Show solution" disclosure widget. Solution blocks can contain prose, math, and rustlab code blocks (which execute and contribute variables to the notebook environment).

## Output format mapping

| Format | Rendering |
|---|---|
| HTML | Exercise: styled `<div class="exercise">` with auto-numbered header. Solution: nested `<details><summary>Show solution</summary>` |
| LaTeX | Exercise: `\begin{exercise}` custom environment with counter. Solution: printed inline (collapsibility not feasible in print) or gathered in an appendix section. |
| PDF | Same as LaTeX |

## Scope rules

- `<!-- exercise -->` captures content until `<!-- solution -->` or the next heading.
- `<!-- solution -->` captures content until the next `<!-- exercise -->`, heading, or a blank line followed by non-indented content.
- Exercises auto-number within the notebook (Exercise 1, Exercise 2, ...).
- A solution block is optional — an exercise without a solution is valid (open-ended prompt).

## Motivation from quantum_lab

Each lesson.md has 3-5 exercises that could be embedded in the notebook:

| Lesson | Example exercise |
|---|---|
| 01 | Add the 4s orbital and count radial nodes |
| 02 | Predict the output state of the H-S-H circuit |
| 03 | Find the detuning that halves the max excitation |
| 04 | Compute the coherent state overlap $\langle\alpha\|\beta\rangle$ |
| 05 | Estimate the AC Stark shift error at $\Delta = 3\Omega$ |
| 06 | Compute $S$ for non-optimal CHSH angle choices |
