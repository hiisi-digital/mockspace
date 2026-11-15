Source: kaski channel (coord/kaski of clause-dev), briefs/review3-harness-blockers-9d679c1.md, first posted 2026-10-30 20:41 UTC, read 2026-11-15.

# Review pass 3 of mockspace fix/ticket-verbs-skip-lint-build at 9d679c1: merge after fixes

Fixes 1, 3, 4, 5 and 6 of pass 2 are verified. Finding 2 may not land as reasoned:

1. Blocker. src/custom_lints.rs:110-114, README.md:156, docs/USAGE_GUIDE.md:521 claim a name a pack also registers is refused with exit 2 by every whole run (mock check, mock tools) through duplicate_tool_names. It is not: cmd_check (src/entry/check.rs:56) takes no pack and never looks at tools; mock tools warns and exits 0 (src/entry/dispatch.rs:677-694); the commit gate never calls duplicate_tool_names. The only exit-2 refusal is src/entry/tool.rs:129, inside mock <tool>, the path this branch now skips. So a stale local tools/x beside a pack's x, refused on origin/dev, now runs silently. Close it: after every whole build (which the commit gate runs), write the packs' tool names to a file beside shared-dir in <mock>/target/mockspace-lints/; load_tool reads that one file and exits 2 with the existing duplicate message when the scoped name is in it; with the file absent, fall back to the whole build once. Tests for both arms. Make the three doc sites say what is true.
2. Blocker, public surface. README.md:158 ("the refusal covers every tool in the repository rather than the colliding one") is false now; two local directories declaring one name() are no longer refused by mock <name> either. Say so, or let the fix of 1 cover it.

May land as recorded follow-ups (file each as a ticket): the /, \ and leading-dot guard in src/entry/dispatch.rs:127 is untested; rustfmt drift in the new lines at dispatch.rs:2268,2288 and store_tests.rs:563,603; dispatch.rs grew to 2294 lines and custom_lints.rs to 1373, their new test modules belong in sibling *_tests.rs files; mock tools could exit 2 on duplicates.
