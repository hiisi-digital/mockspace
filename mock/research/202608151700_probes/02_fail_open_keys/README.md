# Probe 02: the two keys that decide what the numbers mean fail open

`baseline` and `floor` are the keys that decide what every ratio in a report
is a ratio *of*. Neither is checked against the arm set.

- `DataSet::with_baseline` (`bench-harness/src/analysis.rs:276-281`) keeps
  `baseline_idx = 0` when the name is absent.
- `DataSet::with_floor` (`analysis.rs:295-298`) stores any string;
  `floor_mean()` (`analysis.rs:302-310`) returns `None` when absent, and
  `report::generate` (`report.rs:212-215`) then renders raw ratios.

## Result

A one-character typo in `floor` renders a report **byte-identical to
declaring no floor at all**, with `threaded` reading `0.50×` where the
correct spelling reads `0.38×`. The explanatory footnote at
`report.rs:227-239` is emitted only when the floor resolved, so the only
difference between "floor applied" and "floor silently dropped" is the
absence of one paragraph.

## Negative controls

Three, all asserted, and the program aborts if any fails: the correct
spelling must change the ratio, the correct spelling must emit the note,
and `with_baseline` must actually move the baseline. Without these, a probe
showing "A and B look the same" would be equally consistent with the keys
doing nothing at all.
