# Evidence for the `mock bench test` subcommand

Committed artifacts backing `mock/research/202608180722_bench-tree-test-discovery-findings.md`.

- `01_parse_bug_repro.txt`: the negative-control reproduction of the parser bug
  found and fixed while building the fix (the fix's own first draft silently
  read a real passing crate as zero tests).
- `02_real_tree_verification.out`: `MOCKSPACE_REAL_TREES=~/Dev/clause-dev cargo
  test --lib bench::tests::cmd_test_finds_and_runs_a_real_arvo_support_crate --
  --ignored --nocapture`, run against a scratch copy of arvo's real
  `warm-container-shared` crate (15 real tests, independently known-passing
  from a prior session's direct `cargo test --release` run in that crate).
- `03_pre_change_full_workspace_test_run.log`: `cargo test --workspace` on a
  clean `dev` checkout (the change stashed out via `git stash`, run, then
  popped back), verifying the test gate rather than inheriting the brief's
  claimed count. 641 passed, 0 failed (the brief claimed 639; not chased
  further, see the findings file).
- `04_post_change_full_workspace_test_run.log`: the same command with the
  change applied. 647 passed, 0 failed: the 6 new unit tests pass, the new
  real-tree test is correctly `#[ignore]`d in the default run, nothing broke.
