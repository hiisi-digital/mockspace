# Mutations the tests were shown to catch

Each change was made to `src/ref_store.rs`, the tests were run, and the source was restored. The tests were green before and after. 2 November 2026.

| Change | Red tests |
| --- | --- |
| A fetch that cannot reach the remote reads as an empty store | `an_unreachable_remote_is_an_error_and_never_an_empty_store`, `a_remote_that_went_away_after_a_fetch_is_loud_on_the_next_refresh` |
| Commits written without a parent, as v2's `write_round_ref` does (right for a round ref, wrong for a namespace that keeps its history) | `every_write_is_a_commit_on_the_tip`, `a_lost_race_is_rebased_through_the_hook_and_keeps_both_commits`, `a_shallow_writer_adds_to_a_long_history`, `many_writers_at_once_lose_nothing` |
| The merge hook is not asked after a lost race | `a_lost_race_is_rebased_through_the_hook_and_keeps_both_commits`, `a_hook_that_refuses_leaves_the_winner_in_place`, `a_hook_can_change_the_edit_to_fit_the_new_tip` |
| Every failed push is retried, a refusal included | `a_refusal_that_is_not_a_race_is_not_retried` |

A remote that lies about an accepted push is covered by a git shim in `ref_store.rs` (`a_push_the_remote_took_but_reported_failed_is_not_applied_twice`); the `Verify` error itself (the remote does not hold a commit it accepted) has no test, because a local bare repository cannot be made to drop an accepted push.

## Added after the first review pass

| Change | Red tests |
| --- | --- |
| The verify fetch is depth-limited, so a shallow store cannot see its own commit under a newer one | `a_shallow_writer_overtaken_after_its_push_still_knows_it_landed` |
| A push reported as failed is never checked for having landed | `a_push_the_remote_took_but_reported_failed_is_not_applied_twice` |
| The edit's base is not the tip the store held when the call began | `a_write_after_a_stale_read_goes_through_the_hook` |
| A failed push that changed nothing is never retried on transient words | `a_dropped_connection_that_changed_nothing_is_tried_again` |

The reported-failed push and the dropped connection use a stand-in for the `git` command (`RefStore::with_git_program`), a shell script that runs the real git.

## Added after the second review pass

| Change | Red tests |
| --- | --- |
| A fetch follows a remote whose history was rewritten | `a_remote_whose_history_was_rewritten_is_not_followed` |
| A write takes the clone's shared local ref as its base, not the tip this store read | `a_store_reads_what_it_saw_even_when_another_store_on_the_clone_moves_the_local_ref` |
| A commit pushed while the follow-up fetch dropped is forgotten | `a_push_that_landed_whose_follow_up_fetch_also_dropped_is_still_not_applied_twice` |
| A push that dropped on every attempt is reported as a lost race | `a_push_that_keeps_dropping_is_reported_as_that_and_not_as_a_lost_race` |
| `.GIT`, `git~1` and the like are accepted as store paths | `path::tests::paths_that_leave_or_confuse_are_refused`, and the directory backend of the conformance suite |
| An accepted push whose check cannot reach the remote is an error | `an_accepted_push_whose_check_could_not_reach_the_remote_is_landed_and_unconfirmed` |
