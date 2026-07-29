# Bug: `<!-- solution -->` emits unclosed `<details>` in markdown output

Affects rustlab-notebook **0.3.6 and 0.3.7**. Re-tested 2026-07-28 against 0.3.7 —
still broken. This has blocked adoption of the exercise/solution directives in
`quantum_lab` since 2026-06-11.

## Symptom

`rustlab-notebook render -f markdown` opens `<details><summary>Solution</summary>` for
each `<!-- solution -->` directive but never emits the matching `</details>`. An extra
empty `<details>` pair is emitted where the exercise auto-closes. The renderer's own
linter catches it: `rustlab-notebook check --strict` reports `W002`.

## Reproduction

```markdown
## Exercises

<!-- exercise -->
1. **First exercise.** Compute something.

<!-- solution -->
The answer is 42.

<!-- exercise -->
2. **Second exercise.** Compute something else.

<!-- solution -->
The answer is 43.

## Connections

Trailing prose that must NOT be swallowed into a collapsed widget.
```

```
$ rustlab-notebook render nb --format markdown --output out
$ grep -c '<details'   out/soltest.md    # 3
$ grep -c '</details>' out/soltest.md    # 0

$ rustlab-notebook check out --strict
[rustlab:W002] warning: 3 `<details>` opening tag(s) without matching `</details>`
1 warning(s) across 1 file(s)
```

Three opening tags, zero closing tags, for two solutions.

## Why this matters

On GitHub an unclosed `<details>` swallows **the entire remainder of the page** into the
collapsed widget. In the reproduction above, everything from the first solution onward —
the second exercise, the Connections section, and all following content — disappears
behind a disclosure triangle. For a rendered lesson that is the whole back half of the
document.

Because the damage is invisible in the source and only appears once the markdown is viewed
on GitHub, this is easy to ship without noticing.

## Encountered in

`quantum_lab`. The directives are unusable, so all 18 lessons hand-roll their exercises as
plain numbered lists and carry no solutions at all. Recorded in `AGENTS.md` under "Known
bugs" with a standing instruction: *"Do not use `<!-- solution -->` in notebooks until this
is fixed upstream."*

The tracking item in `dev/fable_lesson_updates.md` has been parked across two rustlab
releases now (0.3.6, then 0.3.7).

## Note on `<!-- exercise -->`

Plain `<!-- exercise -->` numbering renders correctly in both formats and is not affected.
One adoption wrinkle worth documenting either way: exercise blocks auto-close only at the
next `<!-- exercise -->` or end of document, so a lesson that ends with
`## Exercises` followed by `## Connections` pulls the Connections section into the last
exercise block. Adopting the directives therefore requires moving `## Connections` above
`## Exercises`. A block that closed at the next heading of equal-or-higher level would
remove that constraint.

## Proposed fix

- Emit `</details>` at the end of each solution block.
- Suppress the empty `<details>` pair emitted at exercise auto-close.
- Consider making `check`'s W002 an error rather than a warning under `--strict`, since
  unbalanced `<details>` corrupts every downstream markdown viewer rather than merely
  looking untidy.
