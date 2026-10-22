# Test history

`cargo mock test` keeps one record per member under `{mock_dir}/test-history/<flavour>/<member>.json`,
committed with the work. It names the fingerprint a member's suite last passed at (`green`), the
fingerprint each test binary passed at (`bins`), and the timing and result of each slow test
(`tests`). This is what happens to those files when branches meet and when a commit carries them.

## Merging

`.gitattributes` sends every `.json` under `{mock_dir}/test-history/` to the `mockspace-test-history`
merge driver, which `cargo mock` registers in the clone and which runs `cargo mock
merge-test-history`. Two branches that both reran a member do not conflict:

- a green record, and a binary's recorded pass, is kept where both sides hold the same one or only
  one side moved it from the base, and dropped where both moved it to different values. Neither
  fingerprint is the merged tree's, so the next `cargo mock test` records it afresh. The driver never
  keeps one side's green over the other's;
- per-test results and timings are unioned. A test one side removed and the other left alone stays
  removed. A test both sides timed takes the later measurement: the side that moved it, else the side
  whose tip commit is later (for a `git merge`), else the longer timing;
- a side that is not a test-history record, two versions, or two members or flavours are refused
  with a message and the path stays conflicted. Do not pick a side by hand: rerun `cargo mock test`
  on one side so both are the current version, then merge again.

## The commit guard

The pre-commit hook runs `cargo mock check-test-history` for a commit that stages one of these
files. A commit that removes the green record of a member while changing none of its inputs (its own
directory, the directories of the members compiled into its tests, the paths its sources read from
outside, the workspace-wide files) is refused, naming the member: a run cut short by a restart or a
full disk leaves exactly that. Rerun `cargo mock test` so the record is made again, or unstage the
file. A merge commit whose nulled green is one the driver's rule drops is not refused; one the rule
would have kept is.
