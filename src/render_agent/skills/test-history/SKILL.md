# Test history

`cargo mock test` keeps one record per member at `{mock_dir}/test-history/<flavour>/<member>.json`, committed with the work: the fingerprint the member's suite last passed at (`green`), the fingerprint each test binary passed at (`bins`), and each slow test's timing and result (`tests`).

## Merging

`.gitattributes` sends every `.json` under `{mock_dir}/test-history/` to the `mockspace-test-history` driver, registered by `cargo mock` in the clone's git config (`cargo mock register-drivers --global` for the user's). It runs `cargo mock merge-test-history`. Two branches that both reran a member do not conflict.

- A green record, and a binary's recorded pass: kept where both sides hold the same one or only one moved it from the base; dropped where both moved it to different values (the next `cargo mock test` records it afresh). The driver never picks between two greens that both moved.
- Per-test results and timings are unioned. A test one side removed and the other left alone stays removed. A test both sides timed takes the later measurement: the side that moved it, else the side that last changed the file later (for `git merge`), else the longer timing.
- A side that is not a test-history record, two versions, or two members or flavours: refused with a message, path stays conflicted. Do not pick a side by hand: rerun `cargo mock test` on one side so both are the current version, then merge again.

## The commit guard

- The pre-commit hook runs `cargo mock check-test-history` for a commit staging one of these files.
- A commit removing a member's green record while changing none of its inputs (its own directory, the directories of members compiled into its tests, the paths its sources read from outside, the workspace-wide files) is refused, naming the member. A run cut short by a restart or a full disk leaves exactly that. Rerun `cargo mock test`, or unstage the file.
- A merge commit whose nulled green is one the driver's rule drops is not refused; one the rule would have kept is.

## What a run writes

- Writes and removes only the files of the members it ran, in the flavour it ran in, plus members the workspace no longer has (pruned in every flavour's directory).
- A run for `-p one-member`, with or without `--cheap`, leaves every other file byte for byte as found: a blank one a merge left, an older or newer version, one with conflict markers. None is the run's to delete, rewrite or read as a loss.
- A member the run covered and left with nothing to record loses its file, so a failed green does not load back.
- A file of another member that changed or vanished after a run for some other member is a defect to report. Copy a changed one aside first, then write it back from the last commit with `git show HEAD:<path> > <path>`; never `git restore` or `git checkout -- <path>` over it.

## History kept on a git ref

- `[test.history]` in `mockspace.toml` keeps the files on a git ref instead of `<mock>/test-history/`: keys `namespace` (default `test-history`), `remote` (default `origin`), `form` (`branch` or `ref`). A run commits nothing to the tree.
- `cargo mock test` fetches the ref at start and writes one commit to it at the end, pushed with a lease.
- A green may go from a file only when the member's fingerprint now differs from the one it was recorded at, or the workspace no longer has the member. Otherwise the write is refused, nothing is pushed, and the run is worth repeating.
- After a lost race the guard is asked again of the winner's state: a green the winner recorded at the fingerprint this run still has is not dropped; the write is refused instead. When another run pushed first, the files merge as the driver does and the write is retried. A file the winner removed stays removed.
- A ref that cannot be fetched is said aloud; the run goes on with no record and writes none. It is never read as an empty history.
