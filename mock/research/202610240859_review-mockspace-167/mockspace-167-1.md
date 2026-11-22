Source: kaski channel (coord/kaski of clause-dev), reviews/mockspace-167-1.md, first posted 2026-10-24 08:59 UTC, read 2026-11-15.

# mockspace #167 (on #166's branch), review 1, 24 October

Verdict: merge after fixes. Head reviewed d4a6ed2. #166 alone still carries all eight findings of review `reviews/mockspace-166-1.md`.

## The eight findings of review 1
1. Mostly resolved; the generated-arm-manifest branch `src/entry/test_benches.rs:143-152` is untested (mutated to `if false && ...`, all green). See B3.
2. Resolved, but brought B1.
3. Resolved.
4. Partly: the `.git` exclusion test is vacuous (B5).
5. Resolved, 12 cases.
6. Resolved; one integration step vacuous (B4).
7. Resolved on the green path only; the failure path `test.rs:357-360` does not name the skip. Non-blocking.
8. Updated, but the builtin now advises a run that no longer covers the benches (B2).

## Blocking
- B1 `test_benches.rs:56,125`: every `Cargo.lock` under `benches/` is excluded from the digest, committed ones too. `bench_rev.rs:15-17,78-85` makes committed lockfiles the consumer's way to pick the framework commit. Reproduced: committed `arms/a/Cargo.lock`, changed the bench-core rev, digest did not move. Contradicts `help.rs:293` and `USAGE_GUIDE.md:270-274`. Fix: exclude only untracked lockfiles (`git ls-files --cached`), test that a committed lockfile moves the digest.
- B2 `src/render_agent/templates.rs:973` ships "Run `--full --no-cache` once at the end of a piece of work and once after merging the trunk", but `decide` at `test.rs:213-218` reads only `--benches` and `-p`; `--full`, `--no-cache` and `--plain` all skip the benches. Fix: `--full` (or `--no-cache`) implies the benches, the module doc saying they are "selected like the members"; or the builtin, help and USAGE_GUIDE name `--benches`. Reviewer prefers the first.
- B3 tests sample what the digest walks: the generated-arm-manifest branch untested; `dev-dependencies`, `build-dependencies` and `[target.*]` unasserted (verified by hand only); `the_workspace_inputs_move_the_digest` (`test_benches_tests.rs:179`) covers 5 of 14 file-and-base combinations; loop over the whole `WORKSPACE_INPUTS` for both the mock directory and the root.
- B4 `tests/mock_test_benches_when_moved.rs:185-191` asserts the toolchain pin moves the digest while running with `--benches`, which forces the run; drop the flag (the step passes without it).
- B5 the `.git` half of `build_output_and_version_control_files_do_not_move_the_digest` (`test_benches_tests.rs:145`) cannot fail since `git ls-files` never lists `.git`; delete that half and the dead `OUTPUT_DIRS` entry at line 47.
- B6 the generated arm manifests' text is not digested, only the path dependencies parsed from it; members salt their fingerprint with the generated config and a `SCHEME` constant (`fingerprint.rs:96,125-128`). Fix: hash the generated texts as entries and add a scheme constant.

## Non-blocking
- `test.rs:207-211`: the digest is computed under `-p` without `--benches` too, up to three times a run.
- `path_dependencies`: `workspace = true` resolved against the consumer's root manifests even for packages in another workspace.
- `bench.rs:599` `find_crate_manifests` walks gitignored directories the digest never sees.
- `a_dependency_cycle_ends` asserts nothing.
- `test_benches_tests.rs:198` narrates the review process in source.
- The help omits the manifests and `.config/nextest.toml` from the inputs it lists.
- `templates.rs:962-963` lines much longer than their neighbours.

## Runs
1412 passed, 11 failed at the head; all 11 red on `origin/dev` too (5 `hooks_identity_tests`; 6 `registry_config_key_severity` and `registry_findings_gate`, environment: "failed to run cargo check: NotFound"). Clippy and fmt clean on touched lines.

## Sweep
`pr-review` exited CONTAMINATED for author `Claude <[redacted]>` on the stale branch `claude/homma-game-engine-setup-41nijk` (tip a99e8d5), an ancestor of neither PR nor dev. The same case for mockspace-extra-lints and homma sits in `.claude/skills/pr-review/scripts/unrepairable.sh` under op's ruling of 22 October; mockspace has no row there. The reviewer applied that ruling by analogy; owed: a ledger row, and op deleting the branch. For op.
