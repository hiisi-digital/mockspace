# Seat 003, audit (Fabian Giesen). INTERMEDIATE

Intermediate. Op asked for a wrap-up partway through. Sections 1 to 4 are finished as far as they go. Section 5 records what I had started on op's ruling of 12 November about keeping data refs out of `refs/heads/`. Section 6 lists what is unfinished: the full constructive proposal, the migration recipe, the question list for op and the ranked probe list. None of those is written here as a finished product, and nothing in this file should be read as one.

Gates. Canon gate: no `canon_paths` is declared. What governs is op's intent at `000_op.md:69` plus his words of 12 November appended to `000_op.md` (origin commit 49e70d2: "I don't like using the canonical git tree since forges etc autodiscocer and show them ... any old git tree clone will also clone gigabytes of mock stuff they don't need"). The converged position of seats 001 and 002 pushes the data as `refs/heads/mock/<ns>` (`005_linus-torvalds_reply.md:50`, `006_leslie-lamport_reply.md:52`). Op's later words make that vehicle wrong. The rest of the converged position does not depend on the remote namespace, so I audit it and treat the remote vehicle as reopened (section 5). I am the first reader on that canon fit, and a second read is owed.

Test gate. I started the whole suite (`CARGO_INCREMENTAL=0 cargo test --workspace --no-fail-fast`, git-shook first on PATH, `CARGO_TARGET_DIR=/home/user/target`). When op called the wrap-up it was still running in the long kill-at-each-git-call scenarios: 2017 passed, 0 failed, 54 ignored so far, partial output at `007_probes/suite_partial_output.txt`. My attempt to stop my own run was refused by the permission layer, so it is still running as a process in `/home/user/.worktrees/seat003` (pids 9877 and 9879 when I checked). The coordinator should know before scheduling another cargo process. I read the bodies of the tests the members cite and checked them against source (section 1). I found no tautology in the cited set. The sampled laws are in section 2.

All probes are ad-hoc spikes on git 2.43.0 and git-lfs 3.4.1, committed with their raw output in `007_probes/`, with the scratch path rewritten to `<scratch>`. They prove existence claims and refute universals. They price nothing. Every timing in them is an ad-hoc quick spike, not a bench.

## 1. Claims checked against source, and what held

- Local ref form. `mock/crates/mockspace-store/src/refs.rs:50-54` (`ref_form`, `refs/mock/<ns>` on both sides) and `:56-62` (`branch_form`, `refs/mock/<ns>` locally, `refs/heads/mock/<ns>` on the remote). Held, as 001 says and 004 conceded. 001 cites the range as `:46-62`, which is close enough.
- The proxy record. `refs.rs:16-19`: the proxy accepted `refs/heads/mock/probe` and hung up on `refs/mock/probe` and `refs/notes/mock-probe`. Held. It matters again in section 5, because op's ruling points at exactly the namespace this proxy refused.
- `--no-filters` on every commit path: `instance/files.rs:228`, `instance/backup.rs:320`, `ref_ops.rs:303`. Held, all three lines verbatim.
- `pending.rs:1-23`. The copy is named by a file and is lost to a `git gc` past `gc.pruneExpire`. Held. Op's "Ref-held + cache" is unbuilt.
- The `core.bare` move. `src/data_refs/sparse.rs:198-216` moves `core.bare` into `config.worktree` and sets `extensions.worktreeConfig`. Held. That is the source of red test 2.
- `Refuse` wired into `update`. `src/data_refs/verbs.rs:317`. Held.
- The reftable skip. `tests/refs_backup.rs:325-327`: `eprintln!("skipped ...")` then `return`. Held. It is a test that reports green without running, and it should be `#[ignore]` with a reason so it counts as not run.
- The LFS test. `tests/refs_consumer_migrations.rs:171-183` asserts `!held.starts_with("version lfs-like")`, which means it demands the bytes. Held: it pins the option op refused at `000_op.md:45`. One detail neither seat names: the fixture's filter is a home-made "lfs-like" clean filter, not git-lfs, so the test does not exercise git-lfs at all.
- The namespace prefix. `src/data_refs.rs:95-96` refuses only "declared twice", and `validate_namespace` (`refs.rs:80-`) admits `/`. Held, so the `a` against `a/b` D/F defect of 005 r2 is real. 005 cites the validator at `:90-100`, but the function starts at `:80`.
- The relative paths. 24 `Cargo.toml` under `mock/research`, 13 of them with `../../..`. I recounted this myself and it held. That fact decides placement (section 3).

## 2. What broke

### 2.1 "Immune to every verb run in the main worktree" is a sampled law, and false

The converged shape is a data worktree nested at a same-depth path that no commit has tracked, unhidden (`005_linus-torvalds_reply.md:36`, `006:19`). Its immunity was asserted from q4 (four verbs) and r1/r3. I ran 45 verbs per arm, each in a fresh repository, in the main worktree (`007_probes/a1_main_worktree_verb_matrix.sh`, `a1_output.txt`). The arms are U (unhidden, the converged shape), H (hidden by the common `info/exclude`) and T (hidden by a tracked `.gitignore` line on the code branch). The verbs cover checkout, switch, restore, reset (hard, keep, merge), stash (-u, -a, pathspec), clean (all six f/x/X combinations), merge, cherry-pick, revert, rebase, am, bisect, read-tree, checkout-index, sparse-checkout, rm --cached, gc, worktree prune, submodule update, and moves to and from commits that track the data path.

What survives: every move to and from a pre-migration commit in all three arms. On that point the converged position holds over the whole matrix, not just the sample.

What loses, in the main worktree with no engine involved:

- U: `clean -ffd` and `clean -ffdx`. The first has no `x`. An unhidden nested repository is untracked content, so double-f clean takes it whether or not ignored files are included. Neither seat lists `clean -ffd`.
- H and T: `clean -ffdX` and `clean -ffdx`.
- All arms: a commit that tracks a regular file at the data path. `checkout -f` and `reset --hard` destroy the edit in U. In H and T, plain `checkout` and plain `merge` destroy it silently too (a1 rows "polluted", detail in `a2_output.txt`). Git writes straight through a nested worktree's `.git` file: an untracked nested repository is not a submodule boundary to unpack-trees, and the overwritten file shows up as ` D N.md` in the data worktree after the way back.

That last row is the loss path neither member saw. 006 p3 probed only the gitlink kind of pollution and concluded "pollution, not loss". A regular file is the likely kind. In this workspace's own workflow it is produced by an agent in a fresh linked worktree where no data worktree has been instantiated yet, writing research at the data path and running `git add -A`. The disjointness rule ("a path no commit tracks") is therefore not a property any one clone controls. 005 makes it a declaration lint plus a pre-commit and pre-push refusal (`005:13`), but a hook in one clone cannot stop a commit made in another. The rule is the old-commit collision moved to future commits.

### 2.2 The gitlink pollution is a tooling break for everyone, not tidiness

006 rates `git add -A` recording a 160000 entry as a "tidiness cost" (`006_leslie-lamport_reply.md:44`). It is worse than that. Once pushed, that unmapped gitlink makes `git submodule update --init`, `submodule status` and `submodule foreach` exit 128 in every clone, and a real submodule of the consumer is then not populated (`007_probes/a8_output.txt`: `submodule update --init rc=128 lib populated=`). That breaks op's "git's tooling ... unchanged" for any consumer that has submodules. It is an argument against U.

### 2.3 The linked-worktree story is incomplete in both directions

`a3_output.txt`, with a nested data worktree in linked worktree w2:

- H loses on plain `git worktree remove w2`, as q1 showed. U refuses plain remove and loses on `--force`.
- Nobody tried `git worktree lock`. Locking the parent worktree when the engine nests data in it makes plain remove and `--force` both refuse ("cannot remove a locked working tree, lock reason: holds data worktree mock/rdata"). Only `-f -f` gets through. That is git's own vocabulary and git's own gate, and it works for both H and U.
- `git worktree move w2 w3` silently orphans the nested data worktree. Its admin dir still points at the old path, and the next `git worktree prune`, which `git gc` runs on its own after `gc.worktreePruneExpire`, deletes the admin dir ("Removing worktrees/rdata: gitdir file points to non-existent location"). The bytes survive. The index and the worktree identity do not. A lock on the data worktree itself, not just its parent, keeps prune off it. Neither seat ran `move`.
- Moving the whole clone with `mv` breaks every nested data worktree until `git worktree repair <paths>` runs. After that it is fine (`a3`, "mv clone; repair").

### 2.4 `update = git pull` loses ignored files

Both seats make `update` the person's `git pull` (`001:117`, `002:145`, agreed at `005:51`). In the data worktree, an incoming commit that tracks a path the local clone has as an ignored file overwrites it silently, under both `pull --no-rebase` and `pull --rebase` (`a11_output.txt`: `output.txt=[theirs]`, the local unique bytes gone). `git merge --no-overwrite-ignore` refuses. `git pull` has no such option in 2.43 ("unknown option"). The copied-index snapshot recipe (`005:53`, `006:37`) does not hold ignored files (`a11`: "snapshot tree holds output.txt: 0"). Adding `ls-files -o -i --exclude-standard` with `add -f` does hold them. So `update` has to be `fetch` plus `merge --no-overwrite-ignore`, or a pre-check of incoming paths against present ignored files, and the snapshot needs the ignored files at those paths. Research directories hold generated `output.txt` and `Cargo.lock` (ticket `data_refs_commit_takes_the_engine_s_own_edits_with_it` records cargo rewriting 24 of them), so this is not exotic.

### 2.5 Checking out the shared name breaks fetch in a bare clone

005 r3 puts the first worktree's data on `mock/research` itself. In a bare clone kept current by `+refs/heads/*:refs/heads/*`, every fetch then dies whole, and the code branches stop updating too (`a13_output.txt`: "refusing to fetch into branch 'refs/heads/mock/research' checked out at .../w1/mock/rdata", `dev updated=no`). That is red test 2's failure class, recreated by the converged shape. The rule that follows: no data worktree ever checks out the shared name. Each one sits on its own local branch.

## 3. The located disagreements

Placement. On the evidence, inside the tree at the same depth. Two independent builds show the out-of-tree shapes break cwd-relative `..` (005 r1, 006 p1), and I recounted the 13 of 24 manifests. 006's narrowing holds: `--manifest-path` invocations work. The common-dir placement is safe against `clean -ffdx` and `worktree remove --force`, which is real. In-tree placement gets most of that back by other means. The lock closes remove and move short of `-f -f` (2.3). For the double-f clean, a pre-command copy is the cache layer op asked for (2.6 below). On hidden against unhidden, neither seat's answer survives the matrix. Unhidden buys a plain-checkout refusal of the polluted case and `clean -ffd`-only exposure, and it pays with the 2.2 tooling break and permanent `??` noise. Hidden with a lock loses the plain-checkout polluted case. My leaning, not finished: hidden by a tracked `.gitignore` line on the code branch, so that every clone, including one that never ran the engine, ignores the path and `git add -A` cannot pollute it. Add the locks, and close the regular-file case by the untrackable-name option in 2.7 if op accepts the name. One part of this is unprobed and rests only on standard gitignore behaviour: that the tracked `.gitignore` stops `add -A` pollution from a fresh clone. a1's T arm only shows that it behaves like H afterwards.

Per clone against per worktree. Under in-tree placement the working copy is per worktree, as 005 argued. The history is per repository: one remote ref, which `Mock-Ref` binds. These are not competing semantics. They are the two layers, and a12 shows the composition works end to end in a bare clone with two linked worktrees. Per code branch I would reject. It is the pre-migration state again under another name, and the pairing already records which data a landing used.

Bare clone with linked worktrees, end to end. Run in the composed shape (`a12_output.txt`): per-worktree local branches `mock-wt/<ns>/<wt>`, both worktrees and both data worktrees locked, the exclude in the common `info/exclude`. Under both refspec styles: a merge through the data branch's own `.gitattributes` driver, push and pull between siblings, the landing ancestor check, plain, tracking and mirror-style fetch all succeed, red test 2's three moves succeed, `core.bare` is untouched and no `extensions.worktreeConfig` is set. A new worktree has no data until something instantiates it. Not run: the remote vehicle of section 5 in this bare shape.

### 2.6 (placed here because it answers the residue) The pre-command cache

Op wants a cache under every overwriting step. Both seats could only take it at engine verbs, because git has no pre-checkout hook and `reference-transaction` fires after the write. Git does have one hook that runs before almost every index-reading porcelain verb: `core.fsmonitor`. `a14_output.txt`: a hook that copies the data worktrees and then answers "everything changed" captured the edit before `clean -ffdx`, `clean -ffdX`, `reset --hard`, `checkout -f` and `stash -u`. Each of those then destroyed it on disk, and the cache held it. It does not fire for `worktree remove --force` (the lock covers that) or for `rm -rf`. Open costs, all unpriced: it runs on every git command in the repository, a plain `status` included (about 20 to 30 ms per command in the spike, with a full copy). It has to chain to a person's own fsmonitor (the builtin daemon or watchman). It needs a re-entry guard, which the spike has. Content-addressed dedup would be needed to make it affordable. It is best effort, which is what op asked of the cache ("without leaning on it").

### 2.7 Designs neither seat took seriously

- Placeholder gitlink with `.gitmodules` (`url ./`, `update = none`, `ignore = all`): closes double-f clean and regular-file pollution (`a4`). Then one `git submodule init` plus a person's `submodule.recurse=true` rewrites the data worktree's `.git` file to `.git/modules/rdata` and breaks it (`a6`). Dead.
- Bare gitlink with no `.gitmodules`: survives everything in `a7`, including double-f clean, and breaks `submodule update/status/foreach` for every consumer with a real submodule (`a8`). Dead.
- Untrackable directory name: git refuses `git~1` (core.protectNTFS, on by default) and `.git` components at `add`, at `update-index`, and at `checkout -f` and `reset --hard` of a tree crafted with `mktree` (`a9_output.txt`). It is the only in-tree path that no commit from any clone can ever write, which is git itself making the loss impossible. Costs: the name is ugly. Unhidden, it breaks `git add -A` for the whole tree (`a10`: "invalid path 'mock/git~1'", nothing staged), so it must be hidden. A person can still set `core.protectNTFS=false`. The name is op's call.

## 4. Remote vehicle, before op's ruling

Both seats push `refs/heads/mock/<ns>`. That is now wrong by op's words, and section 5 is what I had begun in its place.

## 5. Op's ruling: data refs outside `refs/heads/` (begun)

Probe `a16_remote_ref_outside_heads.sh` / `a16_output.txt`, with the remote holding `refs/mock/<ns>` and clones reached by `file://`:

- A plain clone, `clone --bare` and `fetch --all` in a plain clone get no mock ref and none of its objects. `clone --mirror` gets it, which is right for a backup. This meets op's intent.
- Vehicle that keeps vanilla `git pull` and `git push` working in the data worktree: a dedicated remote `mock-<ns>`, same URL, `fetch = +refs/mock/<ns>:refs/remotes/mock-<ns>/<ns>`, `push = HEAD:refs/mock/<ns>`, `tagOpt = --no-tags`, and per-worktree local branches whose `branch.<b>.remote` is `mock-<ns>` and `branch.<b>.merge` is `refs/mock/<ns>`. Plain `git push` and `git pull` in two clones both worked, and status tracks `mock-research/research`.
- Leak: `git push --all origin` from the main worktree publishes `refs/heads/mock-wt/research/a` on the remote. A forge lists it, and the next plain clone fetches the data. That is exactly what op ruled against, through a vanilla verb.
- Remote loss path: `git push --mirror` from any clone, including an opted-in one that keeps the data under `refs/remotes/`, deletes `refs/mock/<ns>` on the remote. It is recoverable from any opted-in clone, but it is a loss on the remote. Keeping an identity mapping `+refs/mock/<ns>:refs/mock/<ns>` in opted-in clones would make their mirror pushes carry it. That is unprobed, and plain clones would still delete it.
- Alternative: a data worktree whose HEAD is a symref straight onto a non-heads ref. `git status` reads "On branch refs/mock/research" and `git commit` moves the ref, so nothing reaches `push --all`. Bare `git pull` refuses ("You are not currently on a branch"), so update and push become engine verbs over explicit refspecs.
- Unresolved: the proxy recorded at `refs.rs:16-19` hung up on `refs/mock/probe`. If the hosts op uses behave like that proxy, `refs/mock/*` cannot be the remote namespace there, and op's intent then needs a host check before a vehicle is chosen. I did not probe any forge.

The fork for op, as far as I got: a local branch per data worktree (vanilla pull and push, and `push --all` leaks) against a HEAD symref onto `refs/mock-wt/...` (no leak, and pull and push only through the engine). Neither is finished.

## 6. Unfinished

- The constructive proposal in full: the ref layout under section 5, each engine verb as git commands, the snapshot and user-cache recipe (copied index plus ignored-at-incoming-paths, leaf-named ref per 005 r2, fsmonitor layer), and the locks.
- Migration from v1 (data tracked on dev) and from the shipped v2 (local `refs/mock/<ns>`, excluded instance at the old path, sparse, `config.worktree`, `refs/mock-backup/*`, pending file) with no data loss. Neither is designed here. One constraint I can state: `refs/mock-backup/<ns>` must be kept and not renamed in place (004 p2), and the pending commits must be made ref-reachable before anything else.
- The test matrix as a contract: every verb row of `a1`, `a3`, `a11`, `a14` and `a16`, crossed with the clone shapes (normal, linked, bare plus linked) and with hidden or unhidden. Today's suite tests four rows (`tests/refs_daily_history.rs:106-111`).
- The questions for op and the ranked probe list, owed by the seats after me. Highest ranked by what they would change: whether forges accept `refs/mock/*` (decides section 5); an fsmonitor cost bench in the harness; the tracked-`.gitignore` pollution check; the template loader census; reftable on git 2.46 or later; LFS through a real server under the section 5 vehicle.
