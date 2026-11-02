# Test history

`cargo mock test` keeps one record per member under `{mock_dir}/test-history/<flavour>/<member>.json`,
committed with the work. It names the fingerprint a member's suite last passed at (`green`), the
fingerprint each test binary passed at (`bins`), and the timing and result of each slow test
(`tests`). This is what happens to those files when branches meet and when a commit carries them.

## Merging

`.gitattributes` sends every `.json` under `{mock_dir}/test-history/` to the `mockspace-test-history`
merge driver, which `cargo mock` registers in the clone's git config (and, with `cargo mock register-drivers --global`, in the user's) and which runs `cargo mock
merge-test-history`. Two branches that both reran a member do not conflict:

- a green record, and a binary's recorded pass, is kept where both sides hold the same one or only
  one side moved it from the base, and dropped where both moved it to different values. Neither
  fingerprint is the merged tree's, so the next `cargo mock test` records it afresh. The driver never
  picks between two greens that both moved;
- per-test results and timings are unioned. A test one side removed and the other left alone stays
  removed. A test both sides timed takes the later measurement: the side that moved it, else the side
  that last changed the file later (for a `git merge`), else the longer timing;
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

## What a run writes

`cargo mock test` writes and removes only the files of the members it ran, in the flavour it ran in,
and the files of members the workspace no longer has. A run for `-p one-member`, with or without
`--cheap`, leaves every other file, including its own member's in the other flavours,
byte for byte as it found it: one a merge left blank (no green, no tests, no binaries),
one of an older or a newer version, one holding conflict markers. None of those is the run's to
judge, so none is deleted, rewritten as the current version or read as a loss. A member the run
covered and left with nothing to record loses its file, so a green a failure took away does not load
back. A file is pruned for want of its member only when the workspace has no such member, and then in
every flavour's directory.

A file of another member that changed or vanished after a run for some other member is a defect to
report, not the record doing its job. `git restore <path>` brings it back.

## History kept on a git ref

`[test.history]` in `mockspace.toml` (optional keys `namespace`, default `test-history`; `remote`, default `origin`; `form`, `branch` or `ref`) keeps the files on a git ref instead of under `<mock>/test-history/`, so a run commits nothing to the tree. `cargo mock test` fetches the ref when it starts and writes one commit to it when it ends, pushed with a lease. The commit guard does not see a ref, so the writer asks the same question: a green may go from a file only when the member's fingerprint now differs from the one the green was recorded at, or the workspace no longer has the member. Otherwise the write is refused, nothing is pushed, and the run is said to be worth repeating (a run cut short or a flaky test leaves that). A green the winner of a lost race recorded is kept by the merge. When another run pushed first, the files are merged as the merge driver does and the write is tried again. A ref that cannot be fetched is said aloud and the run goes on with no record and writes none; it is never read as an empty history.
