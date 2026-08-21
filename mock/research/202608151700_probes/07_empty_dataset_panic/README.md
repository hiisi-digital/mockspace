# Probe 07: `report::generate` indexes an empty variant list

`report::generate` opens `let base = ds.baseline();` (`report.rs:17`), and
`DataSet::baseline` is `&self.variants[self.baseline_idx]`
(`analysis.rs:312-314`). The two emptiness guards in the function
(`report.rs:36`, `report.rs:42`) are `len() > 1` and sit **after** the index.

`DataSet::from_samples` filters by mode, so any samples set that yields no
variants panics. Confirmed three ways: a mode mismatch, no samples, and the
garbled mode field probe 03 produces from a sheared CSV row.

This is reachable from `mock bench report`, the cheap command a person runs to
re-render a report from a committed CSV without re-measuring. The observed
result there is an index-out-of-bounds panic rather than a sentence naming the
file and the mode it found nothing for.

## Negative control

The same call with a matching mode renders an 82-line report. Without it, the
panics would be equally consistent with a constructor that never works.
