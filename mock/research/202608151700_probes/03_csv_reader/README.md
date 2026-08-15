# Probe 03: the samples reader cannot tell absent from zero

`load_samples_csv` (`bench-harness/src/sample.rs:108-141`) parses every
column with `unwrap_or(<zero>)`. Zero is a meaningful value in each of them,
so a garbled, sheared or truncated row is indistinguishable from a row that
genuinely measured zero.

## Four cases, all confirmed

| input | result |
|---|---|
| one garbled `algo_ns` cell | that arm reads **0.0 ns**, which the report calls the fastest |
| a pre-digest CSV | every `digest` reads **0**, so every arm's digest agrees |
| an arm name containing a comma | every column shears by one; `e2e_ns` reads 0.0 |
| a row truncated mid-write | kept, with the missing tail zeroed |

Rows with fewer than ten fields are dropped at `sample.rs:117` without a
count, so a partial write also loses rows silently.

The pre-digest row matters to this round specifically: change 1 makes the
digest load-bearing for the first time, and a comparison of zeros agrees.

## Negative control

The same reader on well-formed rows must return the written values. Asserted;
the program aborts otherwise. Without it, "the corrupted row read as zero"
would be equally consistent with a reader that returns zero for everything.
