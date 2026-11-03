# Sketching and spikes

A sketch answers a feasibility question (does it compile, does the trait solve, does the shape work, is the feature gate required) with **WORKS**, **FAILS** or **INCONCLUSIVE** and a written record. Invoke before the first probe.

## Where

- `{mock_dir}/research/sketches/<YYYYMMDDHHMM>_<topic-slug>/`, one directory per question.
- A real cargo crate with its own `Cargo.toml` and `src/`, or loose `.rs` files, plus a `FINDINGS.md`.
- Never `/tmp`, a scratch directory, or a bare command whose output scrolls past. If the result changes what gets built, it is a sketch and it is committed.

## Toolchain

- A sketch builds under the repository's pinned toolchain. `rustc` resolves its toolchain from the working directory, so a probe run from a temp directory uses a different compiler.
- Run from inside the repository. A sketch crate inside `{mock_dir}/research/sketches/` inherits the pin.

## FINDINGS.md

```markdown
# Sketch findings: <the question, as a statement>

**Date:** YYYY-MM-DD
**Outcome:** WORKS / FAILS WITH <the error> / INCONCLUSIVE
**Unblocks:** what can now proceed, or what is now known to be closed

## Hypothesis
What was believed, and what would settle it.

## What was tried, in order
Each attempt, failures included, with the actual compiler output quoted.

## The result
What is now established, stated so a reader who does not run the code can rely on it.

## What is NOT established
What the sketch did not test, and what would still surprise you.
```

## Discipline

- A passing sketch proves nothing until shown able to fail. Break the thing deliberately, confirm the sketch notices, record that you did.
- Quote the compiler; do not summarise it.
- Failures are results and are kept. Record the wall and what it is made of.
- Sketches are committed and never deleted. A sketch later shown wrong is renamed `*.deprecated.md` or superseded by a new sketch that cites it.
- Reference a sketch by its directory name, never a commit hash.

## Boundary against benching

- A question needing a timer, an iteration count, or the words "faster" or "cheaper" is a bench in `{mock_dir}/benches/` (the `benchmarking` skill).
- A question with both halves splits: feasibility is a sketch, measurement is a bench. Never answer a performance question with a hand-rolled timing loop in a sketch.
