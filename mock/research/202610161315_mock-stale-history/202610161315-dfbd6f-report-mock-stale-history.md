Source: kaski channel (coord/kaski of clause-dev), att/20261016T131535121810200-mock-stale-history-dfbd6f/report-mock-stale-history.md, first posted 2026-10-16 13:15 UTC, read 2026-11-15.

# mock-stale-history

PR: https://github.com/hiisi-digital/mockspace/pull/153 (`fix/stale-test-history` -> `dev`, one commit `c8eca23`)

## Cause
`suite::run` writes the nextest tool config from `plan.heavy`, which is read from the
test history alone. A history entry for a deleted binary became a
`(binary_id(=x) & test(=y))` override; nextest rejects it ("operator didn't match any
binary IDs"). History was only ever pruned of quick tests, never of tests that no longer exist.

## Fix
- New `src/suite/live.rs`: `list()` runs `cargo nextest list --message-format json` for the
  members that run, `parse()` turns it into binary -> test names. If listing fails nothing is
  dropped (the run reports its own trouble).
- `Plan::retain_live` (select.rs): drops heavy tests and skips that no longer exist, in any flavour.
- `History::prune` (history.rs): for the members listed, drops tests/binaries that no longer
  exist from the run's flavour; applied before `save`, so the file on disk loses them.
- Docs: USAGE_GUIDE, `help test`, and the rendered agent rule in `render_agent/templates.rs`
  (v1 keeps no files under `agent_builtin` about this) say so.

## Reproduction (kaski dev f7227be, `cargo mock test -p kaski-village --cheap`)
Before (launcher's pin = mockspace dev ad523ec): nextest fails before running anything:
`error: operator didn't match any binary IDs` on the generated
`(binary_id(=kaski-village) & test(=...)) | ...` override; `mock test: 1 of 1 tree(s) failed`, exit 1.
After (launcher pinned to `fix/stale-test-history`): plan reports 25 heavy tests (was 26 with
the gone one), 524 tests run, `mock test: 1 tree(s) green`, exit 0. The run rewrote
`mock/test-history/default/kaski-village.json` and `"kaski-village::example/figure_preview"` is gone from it.
Run log: /home/user/repro-before.txt, /home/user/repro-after.txt (container only).

Container workarounds (kaski clone only, nothing pushed to kaski): `mockspace.toml` had both
`[lint-crates]` lines commented out and `mockspace_branch` set to my branch; git global
`url.https://github.com/.insteadOf ssh://git@github.com/` because ssh is unreachable. `--engine <path>`
did not work: it makes lint-rules a path dep next to kaski's git deps (two copies, E0308).

## Tests (mockspace, `cargo test --lib suite::`)
Shown failing against no-op stubs (5 failed / 61 passed), saved in /home/user/red-run.txt:
- history::a_gone_binary_is_forgotten_and_a_present_one_keeps_its_entry: FAIL then pass
- history::a_gone_test_in_a_present_binary_is_forgotten: FAIL then pass
- history::the_pruned_record_is_what_a_save_leaves_on_disk: FAIL then pass
- select::a_heavy_test_whose_binary_is_gone_is_neither_planned_nor_skipped (also asserts no
  override in `tool_config`): FAIL then pass
- live::the_listing_gives_each_binary_its_tests: FAIL then pass
- passing both times (guards): history::members_that_did_not_run_are_left_alone,
  select::a_heavy_test_that_still_exists_stays_planned, live::what_is_not_a_listing_is_nothing_known
After the fix: suite 66 passed, 0 failed. Whole lib: 842 passed, 5 failed, 10 ignored; the 5 are
`bootstrap::hooks_identity_tests::*`, failing identically on unmodified dev (checked by stash), unrelated.

## What kaski needs
Nothing in kaski's files: its launcher pins `mockspace_branch = "dev"`, so once PR 153 merges the
next engine rebuild picks it up. Then commit the pruned `mock/test-history/default/kaski-village.json`
(the next `cargo mock test` rewrites it; or delete the `figure_preview` entry by hand now).
Not verified here: the `mockspace-extra-lints` pack (ssh, unreachable from the container).
