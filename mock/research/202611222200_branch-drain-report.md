# Draining the mockspace and mockspace-extra-lints branches, 22 November 2026

Job ws-v2drain, plan job 2B (`.shared/research/2026-11-22-pre-work-tooling-plan.md`). Refs read after a fetch with every remote branch listed: mockspace `origin/dev` 724b359 at the start, 1024e18 after the two landings; mockspace-extra-lints `origin/dev` 217cb11. `feat/v2` and `work/mockspace-v2-scenarios` were not touched (lane ws-v2).

## What was done

- Landed on mockspace `dev`: eight research records from the unmerged branches (#263, 008fcba), and the close `--take` fix from `salvage/tooling-track-close-takes-its-topic-0bd2def`, with the sha-minute fallback for rounds closed before `close_at` existed and a test for it (#264, 1024e18).
- Every deletion went through `forge-route delete-branch`. The helper container has no forge caller token, so each one is queued in `.git/stale-branches.pending` for a Kaski_Coord sweep; none is deleted yet. Ticket bumped: `landed_and_migrated_branches_stay_on_the_remote_looking_unla`, `forge_route_delete_branch_refuses_a_branch_the_pr_fallback_s`.
- Method that found what `git cherry` could not: dev took most branches as squash commits, so every commit reads unmerged. Tree equality against `git log --first-parent origin/dev`, the aggregate patch id of the whole branch against each dev commit, and per-file blob history settled them. Two review passes over three batches, and a third pass was not needed.

## mockspace, per branch

Retired, content on dev (queued for delete): fix/bench-path-variants, fix/bench-variant-double-shape, fix/cargo-mock-renki-rc3, fix/forbidden-imports-exported-macro-body, fix/git-plumbing-verbs-author-no-message, fix/one-tool-run-reads-a-peer-record, fix/quiet-dependency-build-warnings, fix/target-dir-from-cargo-config, work/channel-port, work/disasm-cross-platform, work/land-bench-stage-review-3, work/land-the-m1-hash-mix-run, work/land-stray-record, work/one-copy-plan, work/sketch-a-bench-arm-that-measures-a-build, work/sketch-edges-across-source-directories, feat/bench-stage, bench/macos-aarch64-apple-m1/20261115T2153Z, work/bench-arm-without-target-path, work/pack-follows-the-consumer-lock, work/proc-macro-in-closure, work/ticket-only-gate, work/mockspace-refs-update.

Retired, landed by squash (tree equal or aggregate patch equal with a dev commit): feat/debug-symbols-only-where-needed, feat/registry-key-paths, fix/a-selective-run-keeps-other-records, fix/bench-one-layout, fix/close-takes-only-its-round, test/generated-docs-stay-put, work/bench-scope-3, work/cargo-mock-renki-rc2, work/builtins-teach-the-harness, work/extension-carriers, work/field-range, work/generated-crates-once, work/generated-outputs-are-ephemeral, work/merge-automation, work/round-adds-a-crate, feat/test-history-merges, work/tooling-track-near-copies, work/tooling-track-refs-table.

Retired after the review of the lint and gate batch (dev holds each: identical tree in a dev squash, except the rival hook fix, which dev replaced): feat/gate-bounded-by-src-dirs, fix/loud-error-on-a-malformed-lint-table, fix/register-forbidden-import-hooks, work/lint-pack-reuse, work/lint-reads (dev later removed that mechanism on purpose, 9c36cfa), work/mockspace-lint-one-pass, work/registry-gate-scoped, work/guard-drops-an-untracked-file, work/homma-shook.

Retired after the second review: work/repo-index (muisti replaced it, and its index config walks into the workspace), feat/date-and-time-field-types (dev took it in e6f3b26 and a1179f8), salvage/ms-probe-20261108T115337Z (a dirty probe tree that deletes 183 lines of dev tests), salvage/tooling-track-close-takes-its-topic-0bd2def (landed, above).

Retired as the refs pieces: work/mockspace-refs-e2e, work/mockspace-refs-letkajenkka (every touched file's final blob is in feat/v2 history), work/mockspace-refs-fetch, work/mockspace-refs-migrate, work/mockspace-refs-registry (also on dev by tree equality). `git cherry` against feat/v2 reads unique for all of them because the history was rewritten; the blob check is the proof.

Research branches: research/data-ref-architecture-{seat001,seat002,seat003,plan} are ancestors of research/data-ref-architecture; the record landed (only 000_op.md was missing on dev, the topics were already on dev inside their closed rounds); all five queued. Duplicates: work/test-bench-scope (same sha as work/bench-scope-3) and work/mockspace-lint-one-pass-core (ancestor of work/mockspace-lint-one-pass) queued. `work/rules-dedupe` was already gone from the remote.

Left:
- work/commit-type-gates: merges clean and is green, but three blockers: workspace names in the lint, a default type set that refuses research and perf, and consumer tables not yet in place. Ticket `land_work_commit_type_gates_after_its_three_blockers`.
- work/bench-kinds: review 2 findings open, 340 conflict lines with dev, consumer migration order. Ticket `land_work_bench_kinds_after_its_review_fixes_and_conflict_co`.
- feat/the-corpus-index-and-mock-ask: op held it on 23 October, and it contradicts his 5 September ruling. Question ticket `question_may_mockspace_feat_the_corpus_index_and_mock_ask_be`, holder op.
- feat/v2, work/mockspace-v2-scenarios: lane ws-v2.

## mockspace-extra-lints, per branch

Retired (dev holds each ported to the reorganised tree; one review pass): work/maintainer-words-from-the-clone (patch equal), work/el-drain (all but a newer README on dev), work/no-admit-list, work/repo-currency, salvage/tooling-track-repo-currency-2-0f9bb14, work/tooling-track-repo-currency-3 (same tree as the salvage tip), work/canon-current, feat/superseded-citation-slang-kind, work/provisional-stamp-lint, fix/unfetched-closure-is-unread (its merge reads clean but defines one test twice).

Left: work/retire-commit-style, a clean green merge that deletes `commit-style`; it may land only after mockspace work/commit-type-gates, since the builtin `commit-subject` replaces it. Bumped ticket `mockspace_extra_lints_has_14_branches_ahead_of_its_dev_and`, which stays open for that one branch.

## Ticket ids

Filed: `land_work_commit_type_gates_after_its_three_blockers`, `land_work_bench_kinds_after_its_review_fixes_and_conflict_co`, `question_may_mockspace_feat_the_corpus_index_and_mock_ask_be`, `mockspace_names_the_workspace_in_tracked_files_and_other_dri`, `land_own_step_deletes_the_branch_it_keeps_and_the_stale_bran`. Bumped: the three tickets named in the brief. None closed: the deletions are queued and not done, one extra-lints branch and two mockspace branches are left, and a helper does not close a problem ticket.

## Tests

`cargo test -p mockspace --lib design_round`: 111 passed, 0 failed (the 110 on the picked commit plus the legacy-round test). The branch reviews ran dev's lib suite at 1837 passed with the 4 known reds (`bench_builds::a_sweep_*` x3, `entry::clean_tests::suggest_subcommand_declines_far_words`), commit-type-gates merged at 1859 passed with the same 4, and extra-lints dev plus retire-commit-style green (460 lib passed, 1 ignored).

## Notes

- The two landings carried no gate: mockspace has no `mockspace.toml`, and `land-own --gated` refuses it; they went with `--reviewed` on a docs-only record change and on a reviewed single commit.
- `canon contradicts` is not run: mockspace has no canon registry.
