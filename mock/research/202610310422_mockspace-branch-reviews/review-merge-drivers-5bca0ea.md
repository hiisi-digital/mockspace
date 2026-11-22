Source: kaski channel (coord/kaski of clause-dev), briefs/review-merge-drivers-5bca0ea.md, first posted 2026-10-30 21:18 UTC, read 2026-11-15.

# Review of mockspace fix/merge-drivers-everywhere at 5bca0ea: merge after fixes

Verified: the generated driver resolves the right git directory in a plain clone, a linked worktree, a submodule and a bare merge-tree; the text-merge fallback keeps the conflict (rc 1, unmerged entries); 1432 lib tests pass.

1. Blocker, src/bootstrap/driver_table.rs:21. Git prefers a clone's local entry to the user config, so a clone still carrying the old `'/home/user/kaski/.git/mockspace/merge-generated' %O...` keeps it until cargo mock runs there again, and never with MOCKSPACE_NO_AUTO_ACTIVATE. Reproduced: a stale absolute local entry plus a conflicting docs/x.md gave merge rc 1, three unmerged entries and the working file plain main with no markers. When register-drivers runs inside a repository, rewrite or unset that clone's mockspace-* local entries that name a path; a test starting from the old command shape.
2. Blocker, public surface. src/render_agent/skills/merging/SKILL.md:7, docs/USAGE_GUIDE.md:122 and :406, src/render_agent/skills/reference-syntax/SKILL.md:121, src/render_agent/skills/test-history/SKILL.md:11, src/render_agent/templates.rs:294 still say the drivers are registered in the clone; none names register-drivers --global, which writes ~/.gitconfig and is documented only in mock help. README.md:221 (Git hooks) lists what mockspace writes into the user's config; the drivers belong there.
3. src/bootstrap/driver_scope_tests.rs:150: the fallback test asserts only conflict markers. Also assert git merge exits non-zero and git ls-files -u is non-empty, and add the clean-merge fallback case.
4. driver_scope_tests.rs: the worktree, submodule and bare cases the module doc claims (merge_driver.rs:38-40) are untested; make them tests.
5. src/bootstrap/merge_driver.rs:156 vs :175: the script is written where resolve_git_dir says (mod.rs:225) and found by git rev-parse --git-common-dir; resolve_git_dir reads a relative gitdir against the cwd, so in a submodule or a relative worktree run from below the root the script lands where the driver never looks. Write with --git-common-dir too.
6. merge_driver.rs:182: a failed script write now registers and reports success; report it.
7. src/bootstrap/registry_driver.rs:80: ConfigTarget concerns every driver; move it to driver_table.rs.
8. driver_scope_tests.rs:18: the git() helper ignores exit status; set GIT_CONFIG_NOSYSTEM; test 2 runs git init against the real global config.
