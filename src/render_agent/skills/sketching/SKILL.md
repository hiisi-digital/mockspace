# Sketching and spikes

A sketch answers a feasibility question: does it compile, does the trait solve, does this shape work
at all, is this feature gate actually required. It produces a **WORKS**, **FAILS**, or
**INCONCLUSIVE** and a written record of how that was established.

Invoke this skill before running the first probe, not after. The failure it prevents is the one that
feels harmless at the time: a throwaway `rustc` invocation in a temp directory that answers the
question, informs a decision, and leaves nothing behind. The decision then rests on something nobody
can re-run, re-read, or check, and the next person re-derives it or trusts a summary of it.

## Where a sketch lives

`{mock_dir}/research/sketches/<YYYYMMDDHHMM>_<topic-slug>/`

One directory per question. Inside it, whatever form the question needs: a real cargo crate with its
own `Cargo.toml` and `src/`, or loose `.rs` files compiled directly, plus a `FINDINGS.md` that is not
optional.

**Never in `/tmp`, never in a scratch directory, never as a bare command whose output scrolls past.**
If the result would change what gets built, it is a sketch and it is committed.

## The toolchain trap

A sketch must build under the repository's pinned toolchain. `rustc` resolves its toolchain from the
**working directory**, so running a probe from a temp directory silently selects a different compiler,
and an answer about the wrong compiler is worse than no answer because it looks like an answer.

Run from inside the repository, or point the output elsewhere and keep the working directory in it. A
sketch crate with its own directory inside `{mock_dir}/research/sketches/` inherits the pin
automatically, which is one more reason to put it there first rather than moving it there afterward.

## FINDINGS.md

The deliverable. A sketch without one is a directory of code nobody can interpret.

```markdown
# Sketch findings: <the question, as a statement>

**Date:** YYYY-MM-DD
**Outcome:** WORKS / FAILS WITH <the error> / INCONCLUSIVE
**Unblocks:** what can now proceed, or what is now known to be closed

## Hypothesis

One paragraph. What was believed, and what would settle it.

## What was tried, in order

Each attempt, including the ones that failed, with the actual compiler output quoted rather than
paraphrased. A diagnostic is evidence; a summary of a diagnostic is a claim.

## The result

What is now established, stated so a reader who does not run the code can rely on it.

## What is NOT established

The part a confident write-up omits. Name what the sketch did not test, and what would still surprise
you.
```

## Discipline

**A passing sketch proves nothing until it is shown able to fail.** A bound that no type satisfies
applies to nothing and compiles cleanly; an assertion over inputs the implementation happens to handle
passes without touching the case in question. Break the thing deliberately and confirm the sketch
notices. Record that you did.

**Quote the compiler, do not summarise it.** The exact error text is the finding, especially when it
names a feature gate, a missing impl, or an unsatisfiable bound. Paraphrase loses the part a later
reader needs.

**Failures are results and are kept.** A sketch that establishes something cannot be done is worth as
much as one that establishes it can, and more than a decision made without either. Record the wall and
what it is made of.

**Sketches are committed and never deleted.** They are the audit trail for why a design went the way
it did. A sketch later shown wrong is renamed `*.deprecated.md` or superseded by a new sketch that
cites it; the original stays.

**Reference a sketch by its directory name, never by a commit hash.** The directory is stable; the
hash changes the moment anything is amended.

## The boundary against benching

A sketch checks whether something is possible. The moment the question needs a timer, an iteration
count, or the words "faster" or "cheaper", it is a **bench** and it belongs in `{mock_dir}/benches/`
under the bench framework. See the benchmarking skill.

A question with both halves splits: the feasibility half is a sketch, the measurement half is a bench,
and each gets its own artifact. Do not answer a performance question in a sketch with a hand-rolled
timing loop; those numbers are unreproducible and cannot decide anything.
