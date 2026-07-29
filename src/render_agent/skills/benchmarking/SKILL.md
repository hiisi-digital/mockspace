# Benchmarking

A bench answers a measurement question: which of these is faster, what does this cost, where is the
band boundary. Any number that feeds a decision comes from here.

Invoke this skill before writing the first timing loop. The failure it prevents is a hand-rolled
`Instant::now()` loop whose numbers look authoritative, cannot be reproduced, and were probably
measuring the wrong thing anyway.

## Where a bench lives

`{mock_dir}/benches/`, scaffolded by `cargo mock bench init`.

```
{mock_dir}/benches/
  Cargo.toml          the bench binary
  src/main.rs         the Routine registration and the workload programs
  bench.toml          which benches exist, their sizes, their variants
  variants/<name>/    one cdylib crate per variant under test
  results/            CSV, meta, and per-size findings. TRACKED, not ignored
```

Commands: `cargo mock bench init` scaffolds, `add <name>` scaffolds a variant, `run [names...]`
builds and measures, `report` regenerates findings from the cache, `list` prints what is registered.

## Why the framework and not a timing loop

Each reason corresponds to a way a hand-rolled measurement lies:

**Per-variant cdylib isolation.** Every variant compiles and loads as its own dynamic library, so no
cross-variant inlining or LTO can blur the difference being measured. Two variants timed inside one
binary share a codegen unit, and the optimiser is free to erase exactly the distinction under test.

**A shared realistic workload.** The harness surrounds the measured call with a common program of
scalar dependency chains, pointer chases, cache pressure and branchy context, so the numbers
approximate a real calling environment rather than an empty loop with a warm cache and an idle branch
predictor.

**Calibrated repetition.** Warmups, cooldowns, passes and per-pass run counts are set by design and
recorded, not guessed per bench.

**An artifact trail.** Every run emits CSV plus meta plus a findings document. A number without one is
not evidence, because nobody can re-run it, and a decision resting on it cannot be audited later.

**Do not re-roll the framework's pieces inside a bench.** Timing helpers, bump providers, stat
collection and validation come from `mockspace-bench-core` and the harness. A bench crate that
reimplements them is reinventing the thing it is sitting inside.

## Making the comparison mean something

**Variants must compute the same answer.** Set `may_differ = false` and `required = true` unless there
is a stated reason otherwise. The harness then cross-validates across seeds, and a mismatch is the
finding: the two implementations drifted, and until they agree the timing comparison is meaningless
rather than merely failed.

**Make sure the difference survives the optimiser.** If a variant's distinguishing work can be
constant-folded away, it will be, and the bench will report that two things are identical when one of
them was deleted. Derive inputs from the harness buffer, and check the emitted code when a result is
suspiciously flat.

**Name what dominates.** If one expensive call inside the measured loop accounts for most of the time,
the ratio between variants is compressed and the absolute delta is the transferable number. Say which
it is.

## The deliverable

Two things, both committed:

**The raw artifacts.** `results/` holds CSV, meta and the harness's own per-size findings. These are
**tracked**, never gitignored, including the runs that produced nothing interesting. The history is
what makes a later regression visible.

**A decision-facing findings note**, in `{mock_dir}/research/`, written for the round that asked the
question rather than for the harness. It states what was measured, the numbers in a table, what the
result decides, and, load-bearing, **what it does not decide**: the shapes not covered, the caveats
that would change the answer, the extrapolation nobody has checked.

A result that only says which variant won has answered less than the question asked.

## The boundary against sketching

A bench measures. A **sketch** establishes whether something is possible at all, and lives in
`{mock_dir}/research/sketches/` with a WORKS / FAILS / INCONCLUSIVE outcome. See the sketching skill.

The moment a sketch reaches for a timer it has become a bench and moves here. A question with both
halves splits into one of each.
