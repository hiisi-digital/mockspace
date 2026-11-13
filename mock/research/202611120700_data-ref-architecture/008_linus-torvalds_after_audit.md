# Seat 001 after the audit

Read: `000_op.md` (the section "During the audit"), `007_fabian-giesen_audit.md` (101 lines, intermediate) and its outputs. Probes for this file are in `008_probes/` (`common.sh`, t1 to t9, scripts with raw output beside them), git 2.43.0, one-behaviour spikes, no benches. I re-ran the audit's a14 (fsmonitor) from its committed script and got the same table. Where I say "audit a1" and so on, I mean their probe, not mine.

Gates. Canon gate: no `canon_paths`; op's `000_op.md:69` plus his new words govern, and those words reopen the remote vehicle, which my 001 and 005 got wrong (they pushed `refs/heads/mock/<ns>`). I am a reader of that canon fit, and a second read is owed. Test gate: the whole suite was run in 001 (section 9: five red). I did not rerun it for this file; nothing in the audit contradicts those numbers, and the audit's own partial run (2017 passed, 0 failed) is of a different, earlier slice.

## 1. The audit's findings, one by one

### 1.1 `clean -ffd` without x: conceded, and wider than the audit says

t1 runs six clean forms against the nested data worktree, unhidden (U) and hidden by the common `info/exclude` (H). An empty `edit survives=` means the file is gone.

- U: `-ffd` and `-ffdx` destroy it; `-ffdX` and every single-f form do not.
- H: `-ffdx` and `-ffdX` destroy it; `-ffd` and every single-f form do not.
- `git worktree lock` on the data worktree does not stop clean (t1 last line: locked, `-ffd`, gone).

So no in-tree arm is free: each loses to one or two double-f cleans. My 003 and 005 listed only `-ffdx`. Corrected text for 005 section 2.1: the in-tree residue is every double-f `clean` that reaches the data (`-ffd` if unhidden, `-ffdX` and `-ffdx` if hidden), plus `rm -rf` and `worktree remove -f -f`. Two f's, typed twice, which is git's own vocabulary for "I mean it", and it is still a loss. The only placement with none of this is outside the tree.

### 1.2 A commit from any clone that tracks a regular file at the data path: conceded, the worst finding

t2, receiving clone with an uncommitted edit in the nested data worktree, an incoming commit that tracks `N.md` at the data path:

- U (unhidden): plain `merge` and plain `checkout` refuse ("untracked working tree files would be overwritten"); `checkout -f` and `reset --hard` destroy the edit.
- H (hidden by exclude) and T (hidden by a tracked `.gitignore` line): all four, including plain `merge` and plain `checkout`, write straight through and the edit is gone; the data file reads `POLLUTION`.
- N (the data path named `git~1`): the polluting commit cannot be made at all (`error: invalid path 'mock/git~1/N.md'` from `git add -f` and `git update-index --add`), and a crafted tree built with `mktree` is refused by `checkout -f` (`invalid path`) and by `merge`.

The audit is right that disjointness is not a property one clone controls. 005 section 1 item 4 made it a declaration lint plus a hook; a hook in the clone that commits cannot stop a clone that has no hook, and the receiving clone is where the file is destroyed. The only mechanism in this whole file that git itself enforces in every clone is the one in the last row (section 1.8). Corrected text: a lint is a detection, not a guarantee; say so wherever the lint is described.

### 1.3 The gitlink breaks `git submodule update` for everyone: conceded

t3: a fresh clone of a repository whose tree holds an unmapped `160000` at the data path (what `git add -A` records when the nested worktree is unhidden): `git submodule update --init` dies `fatal: No url found for submodule path 'mock/rdata' in .gitmodules`, rc 128, and the consumer's real submodule `lib` is not populated. Control arm T (hidden by a tracked `.gitignore` line): no gitlink in the tree, rc 0, `lib` populated. 003 q4 and 005 called the gitlink "pollution, not loss" and a hook could refuse it; both were wrong about the cost, it is a tooling break shipped to every clone. Unhidden is removed from my build. The data path is hidden in the common `info/exclude` and by a tracked `.gitignore` line on the code branch, both (see 1.8 for why both).

### 1.4 `pull` overwrites ignored files: conceded

t4: an incoming commit that tracks `output.txt`, which the local data worktree has as an ignored file with unique bytes. `git merge --no-overwrite-ignore` refuses and the local bytes stay; plain `git pull --no-rebase` writes `theirs` over them silently, and this git has no such option for `pull`. The snapshot recipe I gave in 005 (copied index, `add -A`) holds 0 copies of the ignored file; adding `git ls-files -o -i --exclude-standard -z | xargs -0 git add -f --` holds it (t4: 0 then 1). Corrected build: `update` is `git fetch` plus `git merge --no-overwrite-ignore` of the mirror ref, preceded by a snapshot that includes ignored files. "update is git pull" in 001 and 002 is withdrawn. What remains hazardous is the person typing `git pull` themselves in the data worktree, which is vanilla git and which the layout in section 2 makes inert (bare `git pull` there says "You are not currently on a branch", t7).

### 1.5 The shared branch name kills a mirror fetch in a bare clone: conceded

My r3 put the first data worktree on `mock/research` itself; the audit's a13 shows a bare clone kept current by `+refs/heads/*:refs/heads/*` then refuses the whole fetch. The rule that follows is the audit's: no data worktree is ever on a name that a fetch can target. In section 2's layout no data worktree has a branch at all (HEAD is a symref onto a working ref nobody fetches into), and t7 ends with a bare clone configured that way running `git fetch --all`, rc 0, `dev` advanced.

### 1.6 `git worktree lock`: conceded, confirmed, adopted

t5 reproduces all of it. Lock the parent worktree that holds a nested data worktree: `worktree remove` and `worktree remove --force` both refuse ("cannot remove a locked working tree, lock reason: holds data"); only `-f -f` removes it. A locked parent also refuses `worktree move`. Unlocked parent moved: the nested data worktree is listed `prunable`, and `worktree prune -v` deletes its admin directory ("gitdir file points to non-existent location"). With the data worktree itself locked, the admin directory survives the move and the prune. After `mv` of a whole clone, `git worktree repair <paths>` repairs the pointers and the edit is intact (` M N.md`). Build: the engine locks the data worktree always and its parent worktree when the data is nested in it; the engine's `retire` verb is the only way to unlock, and it snapshots first.

### 1.7 `core.fsmonitor` as a pre-write copy: the mechanism is real, I keep it out of the build

I re-ran a14. A hook that copies and answers "everything changed" captured the edit before `clean -ffdx`, `clean -ffdX`, `reset --hard`, `checkout -f` and `stash -u` (copies with EDIT in cache = 2, each a 25 to 55 ms command), and did not capture before `worktree remove` (cache 0; the lock covers that case). It is the only pre-write hook git has and it works on the verbs that matter. That is conceded.

Why it is not in the build:

1. It runs on every git command in the repository, including `status`. A copy of the data on every command is unaffordable when the point of op's ruling is that the data is gigabytes. The copied-index snapshot (003 q3: 18 ms for 3000 files) is cheaper and still per command.
2. `core.fsmonitor` is one value. A person with the builtin daemon or watchman has set it. The hook that answers "everything changed" disables the person's optimisation for the main tree, and chaining to theirs means running a program whose protocol the engine does not own. Not probed; impression from the documented single-value semantics.
3. Whether the verb is destructive is unknown to the hook (reading the parent process's command line is Linux-only and fragile; not probed).
4. It is unpriced. The only timings are the audit's quick spike and mine. The harness bench with real arms (copy per command, throttled, only for the verbs above, off) is the missing measurement.

What I take from it: it is the answer to op's "cache under every step that overwrites" for raw git verbs, and it is best effort by nature. It stays an experiment gated on that bench. The safety claim of the build does not rest on it.

### 1.8 The `git~1` path: conceded as the only structural answer, with a stated limit

t2 row N and t9. The component `git~1` is refused by `core.protectNTFS` (on by default since git 2.24.1) at `git add`, `git update-index`, and at `checkout -f` and `merge` of a crafted tree, in every clone, by git, with no hook. A pollution commit cannot be made by ordinary tools and cannot be written by git even if crafted. t9, the data worktree at `mock/git~1` on the data branch, hidden by `info/exclude` and a tracked `.gitignore` line:

- `git worktree add` works; status is empty; `add -A` stages nothing; `cargo metadata` of a sketch at `<data>/sketch` with `lib = { path = "../../../crates/lib" }` is OK at the old depth; a real `git commit` in the data worktree works;
- the edit survives `checkout -f` of a pre-migration commit, `reset --hard` to it, `checkout`, `stash -u`, `clean -fd`, `clean -fdx`;
- a double-f clean still takes it (hidden: `-ffdx`, `-ffdX`).

A lesson from my own first run of t9 (a script bug moved `dev` back to the old commit, so the later clean rows ran at an old commit; the committed output is the fixed rerun): a tracked `.gitignore` line is absent at every pre-migration commit, so at an old commit the path is not ignored (the unhidden arm of t1: `clean -ffd` destroys). The tracked line is for fresh clones that never ran the engine; `info/exclude` is for this clone at every commit. Both.

Limits, none probed unless stated: the name is one name (the NTFS check reserves `git~1`; the case variants `GIT~1` and `Git~1` are also refused but collide on a case-insensitive filesystem; impression), so at most one in-tree data directory can use it; `core.protectNTFS=false` is a person's to set; behaviour of a directory named `git~1` on a real NTFS volume is unexamined (impression); the name is ugly and op's call. I take it for the one data directory that has to be in the tree.

## 2. A ref layout that meets op's ruling

Op's words: a forge must not list the data, and a plain clone must not fetch it. The audit's own a16 and my t7 agree on the principle. The layout I probe:

- Remote: `refs/mock/<ns>`, one ref per namespace, outside `refs/heads/`, `refs/tags/` and `refs/remotes/`. The namespace prefix is a declared setting, not a constant, for the reason in 2.6.
- Opted-in clone: a dedicated remote `mock` with the same URL, `fetch = +refs/mock/*:refs/mock/*` and `tagOpt = --no-tags`. The local mirror ref is `refs/mock/<ns>`, the same name the shipped engine already uses locally (`refs.rs:50-62`), and it is never checked out and never a fetch target of a worktree.
- Each data worktree: `git worktree add --detach <path> refs/mock/<ns>`, then a working ref `refs/mock-wt/<enc ns>@<enc worktree>` (leaf names from 005 r2) is created at that commit and the worktree's HEAD is pointed at it by `git symbolic-ref HEAD`. No `refs/heads` entry exists for the data anywhere.
- Verbs are explicit refspecs over those refs: `update` = `git fetch mock` then `git merge --no-overwrite-ignore refs/mock/<ns>` in the data worktree; `commit` = a real `git commit`; `push` = `git push mock --force-with-lease=refs/mock/<ns>:<expected> HEAD:refs/mock/<ns>`.

What t7 shows (all in `t7_output.txt`; the earlier lines `fatal: expected 'acknowledgments', received 'packfile'` appear on every push in this container (`~/.gitconfig` sets `push.negotiate=true`, my guess at the cause) and say nothing about the layout):

1. A plain clone has 0 `refs/mock` refs and 6 objects; `git fetch --all` in it still 0; `clone --bare` 0. `clone --mirror` gets the ref, which is right for a backup.
2. In the opted-in clone, `git fetch mock` brings the mirror ref; `git fetch --all` keeps working (rc 0).
3. The data worktree reads `On branch refs/mock-wt/research@w1`, `git worktree list` shows `[mock-wt/research@w1]`, `refs/heads` holds only `dev`, and a commit in it moves the working ref.
4. `git push --all origin` from the opted-in clone publishes nothing of the data (remote refs afterwards: `refs/heads/dev refs/mock/research`). This is the leak the audit found for the local-branch vehicle (a16), closed because the working ref is not under `refs/heads`.
5. `git merge refs/mock/research` in the data worktree ran the merge driver from the shared config (`a|b|c|mine|theirs`), no engine code.
6. A push of a non-fast-forward to `refs/mock/research` is rejected even without force ("pull before pushing" hint), and `--force-with-lease` with a stale expectation is rejected (`stale info`). Two clones racing are therefore ordered by git, not by an engine compare-and-swap.
7. Bare `git pull` and `git push` in the data worktree both fail with "You are not currently on a branch": inert, so the person cannot take the destructive default by accident, and the engine's verbs are the way in.
8. A bare clone with `+refs/heads/*:refs/heads/*`, a detached worktree of `dev` and a data worktree: `git fetch --all` rc 0, `dev` advanced.
9. `git gc --prune=now` keeps a commit held only by a working ref (`git cat-file -t` prints `commit`).

The remaining two verbs the brief names:

- `push --mirror` from a plain clone deletes `refs/mock/<ns>` on the remote (t7, t8: `deleted refs reported: 1`; the remote then holds `refs/heads/dev` plus `refs/remotes/origin/*`, which mirror also pushed). It is a vanilla destructive verb for any ref the clone does not have. `receive.denyDeletes=true` on the remote does not stop it (t8: still deleted, rc 0): git's receive-pack applies that setting to `refs/heads/` only. A server-side `update` hook that rejects deletion of `refs/mock/*` does (t8: `remote-rejected 1`, ref still present). That helps a self-hosted remote and nothing on a forge whose settings I cannot see. Recovery is any opted-in clone pushing its mirror ref back; the engine's `verify` names a missing remote ref.
- `push --mirror` from an opted-in clone carries `refs/mock/*` and also publishes `refs/mock-wt/*` (t7 last line). Visible leak, caused by the one verb whose meaning is "publish everything".

The audit's dedicated-remote idea, read as a remote name with the same URL, is what I use; its failure was the local branch behind it, not the remote. Read as a second repository with its own URL, the data is an ordinary branch of that repository, fully vanilla, and a plain clone of the code repository fetches nothing. I did not probe it beyond what git does for any repository (impression). It is the fallback if a host refuses `refs/mock/*`.

### 2.6 What is not probed: a real host

`mock/crates/mockspace-store/src/refs.rs:16-19` records that a proxy accepted `refs/heads/mock/probe` and hung up on `refs/mock/probe` and `refs/notes/mock-probe`. Op's ruling is in the namespace that proxy refused. I cannot push to a real forge from here and I did not. So: whether `refs/mock/*` is accepted, listed or hidden by op's hosts is unprobed. Other namespaces are weaker, not stronger: tags are listed by forges and a plain clone fetches all of them. Consequence for the build: the prefix is configuration, and the second-repository variant is the stated fallback.

## 3. The revised build

Same skeleton as 005, with the changes from sections 1 and 2 folded in.

1. Ref model: remote `refs/mock/<ns>`, local mirror `refs/mock/<ns>`, per-worktree working ref `refs/mock-wt/<enc ns>@<enc wt>`, snapshot refs `refs/mock-snapshot/<enc ns>@<enc wt>` (005 r2). Remove `RemoteForm::Ref`/`branch_form` (the remote branch form is now wrong by op's words), the local-branch idea, and the `mock-wt/<ns>/<wt>` heads names. The declaration refuses a namespace that is a prefix of another (005 section 1 item 2).

2. Placement, by one rule: data whose files point back into the tree (this repository's `mock/research`: 13 of 24 manifests) is placed in the tree at the same depth, at the untrackable name (1.8), one per tree; every other namespace is a worktree under the common git directory and read through the resolver, where there is no clean or remove loss surface and no relative-path problem because those files do not point out. The common-dir data worktree is one per repository (002's single copy); the in-tree one is per worktree (005 r3).
   Reading B for the in-tree one if op rejects the name: any same-depth path, hidden by `info/exclude` and a tracked `.gitignore` line, with detection only for pollution (a lint, `verify`, a landing refusal) and the loss in t2 rows H and T accepted as residue for a receiving clone that merges a polluted commit. I do not recommend it; the name is the only row where git does the refusing.

3. Hiding: `info/exclude` and a tracked `.gitignore` line, both. Never unhidden (1.3).

4. Locks: the data worktree always; the parent worktree when the data is nested in it. `retire` snapshots, then unlocks and removes. `git worktree repair` is a `verify --fix` step after a moved clone.

5. Engine verbs, each a few git commands (2 above): `instantiate`, `update` (fetch plus `merge --no-overwrite-ignore`, never `pull`), `commit` (real `git commit`; lints in a `pre-commit` hook that tells the data worktree from the main one by `GIT_DIR` under `<common>/worktrees/`, as seat 002's 004 p3 showed), `push` (lease), `status` (aggregate `git status --porcelain=v2 --branch`), `verify`, `restore`, `retire`.

6. Snapshot before every engine step that writes: copy the worktree's index, `add -A`, plus the ignored files (`ls-files -o -i --exclude-standard`, `add -f`), `write-tree`, `commit-tree`, `update-ref` on the leaf snapshot ref; plus a copy to a one-day user cache directory for what a ref cannot survive (deleting the clone).

7. Pairing: `Mock-Ref: <ns> <sha>` stays. The gate requires the data worktree clean (porcelain empty, ignored files aside) and its commit an ancestor of the local mirror ref after a fetch.

8. Migration. From v1 (data tracked on dev): refuse on a dirty tree; first commit of the data ref is a `commit-tree` of the directory's current tree (no history rewrite, no `subtree split`); push `refs/mock/<ns>`; `git rm` on dev; write the tracked `.gitignore` line; instantiate. From the shipped v2: keep `refs/mock-backup/<ns>` untouched (a different prefix from the new snapshot refs, no D/F collision, 004 p2); make every pending commit reachable from a ref before anything else (op's ref-held pick); reuse the local `refs/mock/<ns>` as the mirror; the old instance's files that differ from base are copied into the new worktree on top of a fresh checkout of the tip, after a snapshot, so they arrive as uncommitted edits and nothing is committed on the person's behalf. Unprobed end to end.

9. Deleted: the sparse machinery and `extensions.worktreeConfig` handling (`src/data_refs/sparse.rs:194-216`), exclude-line toggling (`instance/lines.rs`), the `reference-transaction` hook, the hand-built index and stat cache, walk, hash with `--no-filters` (`files.rs:228`, `backup.rs:320`, `ref_ops.rs:303`), the merge refusal (`verbs.rs:317`, `apply.rs:174-188`), backup, pending, overwritten.

10. Tests owed, red first: the verb matrix of t1, t2, t5, t7, t8 and the audit's a1 (45 verbs per arm) as one table, crossed with the clone shapes (normal, linked, bare plus linked) and the two placements; the LFS test inverted to demand the pointer (`tests/refs_consumer_migrations.rs:171-183`); the reftable test as `#[ignore]` with a reason (`tests/refs_backup.rs:326`); the git-shook test ignored with a reason when the tool is missing.

## 4. What stays open

1. Whether op's hosts accept `refs/mock/*` (2.6). If not: the second-repository variant, or a host-specific namespace. Unprobed.
2. The pre-write cache for raw git verbs (1.7): the fsmonitor mechanism works on the verbs measured and needs a harness bench with real arms and a chaining answer for a person's own fsmonitor.
3. The in-tree name: whether op accepts `mock/git~1` (1.8), its portability (a real NTFS volume, case-insensitive filesystems), and, if not, reading B's accepted residue (3 item 2).
4. The census: how many consumers' data points back into the tree (decides whether any in-tree placement is needed at all), and the loader census for relocating templates (still unread by every seat).
5. `push --mirror` from a plain clone deleting `refs/mock/<ns>`: a server-side hook helps only where the remote is configurable; forge protection for non-branch refs is unknown.
6. Unrun by every seat: reftable on git 2.46 or later, the LFS push side through a server, `git worktree repair` after `git clone --local`, a real forge.
7. The shared-copy concurrency question for the common-dir data worktree (two linked worktrees, an edit in one, a gate in the other): unrun, and only affects the common-dir namespaces.
