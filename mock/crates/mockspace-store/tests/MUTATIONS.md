# Mutations the tests were shown to catch

Each change was made to `src/ref_store.rs`, the tests were run, and the source was restored. The tests were green before and after. 2 November 2026.

| Change | Red tests |
| --- | --- |
| A fetch that cannot reach the remote reads as an empty store | `an_unreachable_remote_is_an_error_and_never_an_empty_store`, `a_remote_that_went_away_after_a_fetch_is_loud_on_the_next_refresh` |
| Commits written without a parent, as v2's `write_round_ref` does | `every_write_is_a_commit_on_the_tip`, `a_lost_race_is_rebased_through_the_hook_and_keeps_both_commits`, `a_shallow_writer_adds_to_a_long_history`, `many_writers_at_once_lose_nothing` |
| The merge hook is not asked after a lost race | `a_lost_race_is_rebased_through_the_hook_and_keeps_both_commits`, `a_hook_that_refuses_leaves_the_winner_in_place`, `a_hook_can_change_the_edit_to_fit_the_new_tip` |
| Every failed push is retried, a refusal included | `a_refusal_that_is_not_a_race_is_not_retried` |

Not covered by a test: a push the remote accepted and then does not hold (the `Verify` error). It needs a remote that lies, and git offers no way to make a local one do it.
