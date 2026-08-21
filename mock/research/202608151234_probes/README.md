# Probes for `202608151234_what-the-consumer-should-write.md`

Three scripts, their raw output, and what each establishes. All three read a
consumer tree that lives outside this repository, so each takes the tree as its
first argument and each output file records what it was run against.

Run against `arvo/mock/benches` at workspace commit `9893fe5`, on
`mockspace` `dev` at `6ab55ee`.

```
python3 p1_driver_table_is_restatement.py  ~/Dev/clause-dev/arvo/mock/benches
python3 p2_sweep_arm_overrides.py          ~/Dev/clause-dev/arvo/mock/benches
python3 p3_point_list_is_written_thrice.py ~/Dev/clause-dev/arvo/mock/benches
```

`p1` establishes that the driver's routine table restates the manifest: 256 live
match arms against 256 size rows, and 47 distinct sections against 47 distinct
(section, bridge type) pairs, so the type is a function of the section name alone.
Memo section 4.

`p2` separates the two questions inside "sweeps share their bench's arm set": one
row in 256 carries a per-point arm override, and 13 sweeps of 49 would carry a
per-sweep one. Memo section 5.

`p3` counts the restatements of one bench's point list across arm attributes, the
manifest and the driver: thirteen writings of the same six integers. Memo
section 4.

## A defect in `p2`, kept in the file rather than tidied away

The first version of `p2` required `variants` to follow `n` immediately in a size
row. Four of arvo's 256 rows carry a comment between them, and **those four are
exactly the rows with a per-point arm override**, so the probe reported that no
row varies by excluding every row that could have varied. It produced a clean,
plausible, wrong answer, and it made the memo briefly claim that per-point
overrides are dead weight when in fact the redesign is right that one bench needs
them.

The fix parses the whole block and raises rather than skipping when a row has no
variants list, so the same failure now stops the probe instead of flattering it.
The comment recording this is kept in the script at the parse site.
