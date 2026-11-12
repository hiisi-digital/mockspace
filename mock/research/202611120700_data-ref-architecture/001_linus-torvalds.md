# Seat 001: the data ref architecture against vanilla git

Gates. Canon gate: no `canon_paths` exists (no `mockspace.toml` at the root or in `mock/`, checked), so the governing text is op's `000_op.md`. The two design rounds are subjects here, not authority. The brief poses no question that op's intent forbids, so the gate passes against `000_op.md:69` (no data loss, machinery that cannot lose data for the ignorant, a cache under every overwrite without leaning on it, git tooling unchanged). I am the first reader on canon fit. A second independent read is owed, and I have not read any other seat.

Test gate: see section 0. It did not come back clean, and what it found changes the question.

Probes are committed beside this file in `001_probes/` with their raw output. Every one is an ad-hoc spike. They prove existence claims about git 2.43.0 behaviour. None prices anything.

## 0. The test gate

Whole workspace, `CARGO_INCREMENTAL=0 cargo test --workspace --no-fail-fast`, git 2.43.0. Result is at the end of this file (section 9), because the run outlived the writing.

Bodies read. What I found, in the surface this panel touches:

1. A test that passes without running. `tests/refs_backup.rs:326-327` prints "skipped: this git cannot move a repository to the reftable backend" and `return`s. That is a green line for the one backend where the design itself says the guard write fails every time (design round `202611110830` line 79, `pending.rs:1-23`). The reftable path has never executed on this machine. Whoever wrote a skip as a return instead of an ignored test chose to inflate the pass count with a test that did nothing. Delete the early return; mark it `#[ignore]` with a reason, so it is counted as not run.

2. A test that pins the opposite of what op ordered. `tests/refs_consumer_migrations.rs:171-183`, `a_file_under_a_filter_is_held_as_the_bytes_the_person_has`, asserts the ref holds the bytes, not the pointer. Op said at `000_op.md:45`: "we use the lfs standard, the pointer ... It should be behaving the same as lfs itself on a branch would". The comment above it (`:167-170`) says it is waiting on a question ticket op has since answered. The test is green and enforces the refused option. The cause is in the source: every hash on the engine's commit paths is `hash-object --no-filters` (`mock/crates/mockspace-store/src/instance/files.rs:228`, `instance/backup.rs:320`, `ref_ops.rs:303`), so no clean filter, no `eol`, no `ident` ever runs on commit. Probe p6 shows the difference: the same file hashes to `8319a000` with `--no-filters` and `cb1c9a20` through git, and a snapshot built by git itself carries the pointer. An assertion message that reads "the ref holds the pointer" on the failing side of a check that wants the bytes is also upside down.

3. Sampled laws. `tests/refs_daily_history.rs:106-111` tests the forced way back with four combinations of `checkout`, `checkout -f`, `reset --hard`. `git switch`, `git restore`, `git stash`, `git bisect`, `git am`, `git checkout-index`, `git sparse-checkout disable/set`, `git worktree remove`, `git clean -ffdx` are not in that matrix. A grep of `tests/refs_*.rs` and `tests/refs_e2e/` for `bisect`, `"switch"`, `checkout-index`, `"am"`, `update-index` returns zero files. The law the design wants is "no git verb that writes the tree loses an instance edit". It is asserted over four of about twenty verbs, and the two verbs that were tried and fail (the two red tests) are exactly where somebody looked. The setups in `tests/refs_consumer_setups.rs:148-170` are tried one at a time (cone, own hook, hooksPath, shook, bare). No crossing: cone with a bare clone, shook with linked worktrees, own hook with reftable.

4. The two red tests are honest and I am not touching them. `tests/refs_daily_history.rs:105` and `tests/refs_consumer_setups.rs:168` are real failures with real reproductions, and the comments at `:90-103` and `:135-141` say what they pin. I report them as the best thing in the suite. Both are consequences of the same decision (section 2).

5. Missing fundamental. Nothing anywhere states the design's central claim as a law: for every git command that writes the working tree of the main worktree, the bytes of every instance file are either on disk or held by a ref afterwards. The design round lists the windows that stay open in prose (round `202611110830` line 81) and "each is pinned by a test". A design whose own text carries a list of unclosed data-loss windows is a design with known holes shipped as a feature.

The tautological-test search (`assert!(true`, a value against itself) found nothing in `tests/refs_*.rs` or `tests/refs_e2e`. That is the only clean line in this gate.

## 1. Breaking the brief

1. "A git ref of its own (`refs/heads/mock/<namespace>`)" is wrong as a description of the local ref. The local ref is `refs/mock/<namespace>` in both forms; only the remote name differs. `mock/crates/mockspace-store/src/refs.rs:46-62`: `ref_form` is `refs/mock/<ns>` on both sides, `branch_form` is `refs/mock/<ns>` locally and `refs/heads/mock/<ns>` on the remote (`src/data_refs.rs:29-30` says the same). So there are two forms (a copy of one mechanism), the local one is not a branch at all, and the reason for the branch form is a hosting limit recorded at `refs.rs:18` (a proxy accepted a push of `refs/heads/mock/probe` and hung up on `refs/mock/probe`). Every git tool that understands branches (`git worktree add`, `git branch`, `git pull`, upstream tracking, `git log mock/x`) cannot take the local ref by name without a prefix. That is the first place the engine leaves vanilla git, and it was an engine choice.

2. The second named round is not about the data ref architecture. `mock/design_rounds/202611111830_topic.reading-references.md:3` says so: "Reading a reference is no part of what a ref holds". It reads a reference standard. Only round `202611110830` bears on this panel, and it is 87 lines of dense paragraphs (several above 2000 characters, lines 73-81), which is the shape of a document that grew by accretion, not by design.

3. The brief lists directories but misses the parts of the engine that the question is about. Around `src/data_refs/`: `src/data_refs.rs` (the declaration), `src/pairing_ref.rs`, `src/bootstrap/hooks.rs` and `src/bootstrap/append_driver.rs` (the hook installer carrying the `reference-transaction` relay, and a merge driver the repo already ships), `src/design_round/round_merge.rs` and `src/append_merge/mod.rs` (git `merge-file` already used as a real merge machinery for rounds). Around `mock/crates/mockspace-store/src/instance/`: the store's top level, `ref_ops.rs` (342 lines, the plumbing commit), `ref_store.rs`, `merge.rs`, `refs.rs`, `retry.rs`, `ref_history.rs`, `commit_reader.rs`, `git.rs`. The brief names 11 of the instance files and not the 9 top-level store modules that decide how a commit is made.

4. "The shipped tree is claims to test." Two claims in op's own record are not shipped at all.
   - The day-long user cache under every overwriting step (`000_op.md:27`, `:35`): `grep -rn "XDG_CACHE\|\.cache\|cache_dir"` over `src/data_refs*` and the store finds only `walk.rs:110` and `:152`, which are about cargo's `CACHEDIR.TAG`. There is no user cache.
   - The ref-held pending copy (`000_op.md:35`, "Ref-held + cache"): `mock/crates/mockspace-store/src/instance/pending.rs:1-23` still names the pending commit in a file and says "a `git gc` that prunes them before the next engine command would lose the copy: git prunes only an object older than `gc.pruneExpire`". Op has already ruled that unsound. It is shipped and unfixed.
   So the guide and the round describe a design op has since rejected in two places, and the tests defend it.

5. "`git commit` of a ref passes through no hook" is true and is a departure the brief does not list: the engine commit is `commit-tree` plus `update-ref` (`docs/USAGE_GUIDE.md:108`: "so no hook runs and it is not signed ... `commit.gpgsign` are all bypassed"). `update` merges nothing: both-sides changes are refused with nothing written (`USAGE_GUIDE.md:424`), and the store "never merges file contents itself" (`mock/crates/mockspace-store/src/merge.rs` header). Op's complaint about merge drivers (`000_op.md:51`) is therefore not a gap in one verb. There is no merge in the engine at all, by construction.

6. The brief says "two red tests pin losses that are still open". I count the red state in section 9. A third failure outside this surface may appear there. Do not assume the red set is two.

## 2. Derivation: how git itself carries "a shared directory with its own history and its own sync, that is not on the main branch"

The need, stated without the current shape: a directory of files, with history, merged by git's merge machinery, pushed and fetched with the repository, not part of the history of the main branch, visible to tools as ordinary files at a stable place in the working tree. The principle I apply: the data structure decides; the code writes itself. Git's own answer to "a second line of history with files" is a branch, and git's own answer to "a branch's files at a second place" is a linked worktree. The current engine is a hand-written second implementation of what `git worktree` does (index, stat cache, add, checkout, status, ignore, merge, hash) plus a pile of guards for where that second implementation collides with the first.

What the engine reimplements, with sizes I counted: the index and racy-stat shortcut (`instance/stats.rs`, 199 lines, design round line 65), the walk and ignore handling (`walk.rs` 177, design line 59), the add and hash (`files.rs` 252), the checkout (`apply.rs` 227), the commit with its compare-and-swap (`ref_ops.rs` 342, `retry.rs`), the sparse and exclude dual files (`lines.rs`, `sparse.rs` 216), the pre-write copy (`backup.rs` 468, `overwritten.rs` 131, `pending.rs` 106, the hook 174). About 3400 lines in the places the brief named, for which git has one command each. Each is a copy of a git mechanism, so each can be wrong where git is right, and two of them already are (the filters, section 0.2; the merge, section 1.5).

Candidates, by category, each against the three questions (loss paths, git-tool compatibility, what an ignorant user can break). Probe references are to `001_probes/`.

### A. The current shape: a branch plus an engine that materialises it in place, hidden by exclude and guarded by sparse

Loss paths. The data files sit at paths that pre-migration commits track, and are hidden from the branch. Git treats a hidden path as expendable. Probe p2 (no engine involved, same shape: files at the old tracked path, hidden by `info/exclude`) shows what git does when HEAD moves to an old commit: plain `checkout OLD`, `checkout -f`, `reset --hard`, `switch --detach` all report success and the edit is gone (`edit_survives=0` on all four). The shipped design answers with skip-worktree via sparse patterns, a `reference-transaction` hook that copies when `ORIG_HEAD` moves, and a backup ref. That closes plain verbs. It does not close the forced way back; that is red test 1 (`tests/refs_daily_history.rs:105`), and the design itself lists four more open windows in prose (round line 81: cherry-pick of an old commit, reset back over an edit made at the old commit, `git rm --cached` then edit, a hung launcher).

Tool compatibility. The sparse checkout needs `extensions.worktreeConfig` (`src/data_refs/sparse.rs:194-214`), which makes a bare clone's HEAD branch unreachable from a linked worktree: red test 2. Cone mode users are switched to full pattern mode (round line 71) behind their back. A user's own `reference-transaction` hook, `core.hooksPath`, git-shook each needed a special case (`src/data_refs/hook.rs:99-151`). LFS and eol filters are skipped on commit (section 0.2). Merge drivers are not consulted (section 1.5). Signing and hooks bypassed (section 1.5). Reftable repositories cannot hold the guard (round line 79) and the test for it is skipped here (section 0.1).

Ignorant user. Every raw git verb that writes the tree is a chance to lose an edit, and the defence is an enumerated list of verbs somebody thought of. Taste test: the special case did not disappear, it multiplied. Each guard exists because the previous guard created a case.

Verdict: this is the wrong data structure. It cannot be made loss-free by adding another guard, because the collision is between two tracked-path semantics, and the guards are all after-the-fact.

### B. Op's example: a worktree per ref, at the same path

Probe p2 and p1 test this with real `git worktree add` and no engine. Results (git 2.43.0):

- nested worktree at the old tracked path, hidden by `info/exclude`: identical loss to A. All four old-commit moves destroy the edit. Plain `checkout OLD` too. Worse, going back to dev deletes the data worktree's files (p1: `NOTES.md` missing), since dev's index is told to remove them.
- same path, not hidden: plain `checkout OLD` and `switch --detach OLD` refuse ("untracked working tree files would be overwritten", the vanilla protection) and the edit survives. `checkout -f OLD` and `reset --hard OLD` destroy it.
- same path, dev tracks a gitlink there (submodule-shaped, p3): same as not hidden. Plain refuses, `-f` and `--hard` destroy.
- data branch checked out in the data worktree of two linked worktrees (p4): git refuses the second without `-f`; with `-f` the second worktree shows `M  NOTES.md` after the first commits, a staged-looking reversal of the first one's edit. Committing there reverts it. One data branch per data worktree, always.

So "a worktree per ref" fixes nothing about the red tests when it lands at the same path. It removes 3400 lines and makes every git tool work, and it does not make the data safe. The same-path collision is the whole problem, and it is a property of the path, not of the materialisation.

### C. A worktree per ref at a path no commit has ever tracked

p2 control row (`other path ex=1`): hidden by exclude, data at `mock/.data/research`, `checkout -f OLD` and `reset --hard OLD` both leave the edit alive. p6: `git clean -fdx` prints "Skipping repository mock/data/research" and leaves it; `git worktree remove` of a dirty data worktree refuses ("contains modified or untracked files, use --force"); only `git clean -ffdx` (the user typed -f twice) removes it, as would `rm -rf`. Those are git's own gates, in git's own vocabulary. Nothing in dev's history can write there, so there is no hook, no sparse file, no `extensions.worktreeConfig`, no exclude-lines-toggled-into-sparse-lines, and red test 2 has nothing left to be about. This is the shape I propose in section 3.

Cost: the data path differs from the path the history tracked, so the tools that read `mock/research/...` read a different path. I have not measured how many readers hardcode it. `src/render_design.rs:577` reads `crate_path.join("README.md.tmpl")` (a template beside its crate), `src/deny.rs:175-195` names a `research` path component, the guide spells paths. That is a resolver change, and the repo is pre-1.0 with the no-shims rule (no alias for the old path).

### D. A separate clone at the data path (`git clone --single-branch -b mock/<ns>`, optionally `--reference` for shared objects)

Fully vanilla, and its gc, index and hooks are its own, which is both the strength and the problem: repo-level config (`merge.<driver>`, `filter.lfs`, `core.hooksPath`, `info/attributes`) does not follow. Op said "we have custom mergers and people might have their own set up". Drivers in the main repo's `.git/config` are invisible to a second repository. In a linked worktree they are shared; probe p5 shows git's own merge in a data worktree running a driver defined in the shared `.git/config` plus a line in the shared `info/attributes`, against a control that conflicts without it. Reject D for that reason. It also duplicates objects unless alternates are set up, and alternates are the classic gc hazard (pruning in the borrowed repo breaks the borrower).

### E. Submodule

A gitlink in dev pinning the data commit, `.gitmodules` pointing at the same URL on `mock/<ns>`. Git protects a populated submodule on plain moves (p3) and not on `-f` or `--hard`. It forces a dev commit for every data change, which op wants gone ("files move without a commit on any branch", round `202611110830`, subject of op's design). The pin would be the pairing, which is a nice property: the `Mock-Ref` trailer (round line 35) is what a gitlink is. But `submodule update` on checkout of an old commit tries to take the directory over, and merge conflicts on gitlinks are opaque. Reject, but note the pairing equivalence: keep the trailer.

### F. git subtree, `read-tree --prefix`

Brings the data history into dev's history. The opposite of the intent. Reject. One use survives: `git subtree split --prefix=<dir>` makes the first commit chain of the data branch from the directory's own past, which is the migration step (section 3, step 8), a vanilla tool replacing `directory-to-ref`'s hand-built history.

### G. git notes, git replace, alternates

- Notes: one blob per annotated commit. The wrong shape for a tree of files with their own history. Reject.
- Alternates: object sharing only, no file layout. Not a candidate. It matters only for D.
- Replace, probe p7: `git replace OLD REP`, where REP is OLD with the data path removed from its tree, makes git present every old commit without the data files. With the data directory at the old path, hidden by exclude, `checkout OLD` and `reset --hard OLD` both leave the edit alive (p7 `edit survives: 1` twice), and commit ids do not change. This is the only same-path option I found that closes the forced way back for real. It carries three costs. A fresh clone has zero replace refs (p7: `refs/replace in clone: 0`); they arrive only by an explicit `fetch origin refs/replace/*:refs/replace/*`. The repository's own comment at `mock/crates/mockspace-store/src/refs.rs:18` records that a hosting proxy hung up on a push outside `refs/heads`, so shipping them through the remote is not safe to assume. And it needs one replace ref per pre-migration commit that tracked the path, regenerated locally on every fetch that brings in an old branch, with a window between fetch and regeneration where the old collision exists again. It is a plausible second reading (section 4, reading B), a fragile one.

### H. Other things git has that I considered and discarded

`git stash`, `git update-index --skip-worktree` by hand, `assume-unchanged` (hides from status, does not protect from writes), `core.sparseCheckout` alone, `sparse.expectFilesOutsideOfPatterns` (the shipped comment at `tests/refs_daily_history.rs:98-100` says it was measured: it keeps the forced ways and turns a plain checkout back into a delete), `post-checkout` (runs after the write, no `pre-checkout` exists), `reference-transaction` (fires for `ORIG_HEAD`, not for the `HEAD` symref move of `checkout -f`, per the same comment), a gitattributes `-merge`/read-only trick (no write protection), a file-system level approach (bind mount, overlay, immutable flag: a platform dependency the repository does not have today, and untestable across the OSes this ships on; impression, not probed).

## 3. What I would build

One sentence: a data ref is an ordinary branch, its directory is a linked worktree of that branch at a path no commit has ever tracked, and the engine does the three things git cannot do for it (declare, snapshot, pair).

Naming and shape, all of it git's:

1. One ref form. The branch `refs/heads/mock/<ns>`, locally and on the remote. Delete `RemoteForm::Ref`, `ref_form`, `--form`, and the `refs/mock/` local ref (`refs.rs:46-62`). The remote could never take the other, so the second form is a copy of one mechanism with no caller.

2. One data worktree per main worktree, on its own local branch. A branch can be checked out once (p4). The first instance in a repository uses `mock/<ns>`; a linked worktree's instance uses a local branch that tracks it, e.g. `mock-wt/<ns>/<worktree-name>`, with `branch.<name>.remote` and `.merge` set so `git pull` in the data worktree is the ordinary command, and push is the explicit `git push origin HEAD:refs/heads/mock/<ns>`. The local branch name is a choice; the one-branch-one-worktree rule is not.

3. A path that history cannot collide with, enforced where it is declared. `data-refs declare` (or the config load) refuses a path that any commit of any ref has ever tracked: `git log --all --name-only --format= -- <path>` non-empty is a refusal that names the commit. Cost of that check on a large repository is unmeasured (section 5, probe 3). The path is op's to name; I will not name it. It must be inside the work tree so tools and relative links still work, and outside every tracked path ever.

4. Hidden by the common `info/exclude`, one line per data path (shared by all linked worktrees, so no per-worktree anything). No sparse file, no `extensions.worktreeConfig`, no cone-mode conversion, no skip-worktree bits.

5. Everything the engine verbs do is a git command run in the data worktree:
   - `instantiate`: `git fetch`, `git worktree add -b <local> <path> origin/mock/<ns>`, append the exclude line, write the snapshot ref once.
   - `update`: `git -C <path> pull` (the user's `pull.rebase`, merge drivers, `.gitattributes`, LFS smudge, hooks all apply because it is the real command). A conflict is a git conflict in a worktree, solved with git's tools.
   - `commit`: a real `git commit` in the data worktree, which runs the shared hooks, honours `commit.gpgsign`, applies filters. The lint gate the engine runs today in front of `data-refs commit` (`design round line 45`) becomes a `pre-commit` hook of the repository, installed through the existing hook machinery, scoped by `GIT_WORK_TREE` being a declared data worktree.
   - `push`: plain `git push` with the explicit refspec; the pre-push stand-down variable `MOCKSPACE_DATA_REF` (round line 47) disappears because the data push is a push of a branch like any other and the hook decides by the pushed ref names.
   - `status`: the engine aggregates `git status --porcelain=v2 --branch` over every data worktree. No stat cache, no `ignored, not held` lines; git says it.
   - `verify`: one check per data worktree: the worktree exists, its HEAD branch is the declared one, `git worktree list --porcelain` agrees, the exclude line is present, no data path is tracked by the main branch (`git ls-files -- <path>` empty), the declared path has never been tracked (section 3 item 3).
   - LFS: nothing to do. The pointer goes through `git add`. Op's ruling at `000_op.md:45` is satisfied by deleting the `--no-filters` code and the test that pins it.

6. The recovery cache, built by git, under every step the engine itself writes with, held by a ref. Before any engine verb that can overwrite files of a data worktree (`instantiate` over an existing directory, `update`, `restore`, migration steps, `verify --fix`), take a snapshot with a temporary index: `GIT_INDEX_FILE=<tmp> git read-tree HEAD; git add -A; git write-tree; git commit-tree -p HEAD`, then `update-ref refs/mock-backup/<ns>/<worktree-name>`. p6 shows this produces a commit containing the modified and the untracked files, with filters applied (the pointer blob, `cb1c9a20`, not the raw bytes). Ref-held, so gc cannot prune it (op's pick at `000_op.md:35`). Also copy the dirty files to a user cache directory with a one-day expiry (op's pick, same place). It replaces `backup.rs`, `overwritten.rs`, `pending.rs`, `stats.rs`, the `reference-transaction` hook and the sparse machinery, all of them. It needs no lock inside a hook, so the reftable problem (round line 79) does not arise: the snapshot is taken by the engine in its own command, outside any transaction.

7. Landing and pairing. Keep `Mock-Ref: <ns> <sha>` (round line 35). The gate reads the data worktree's HEAD and requires: the worktree clean (`git status --porcelain` empty), and HEAD an ancestor of `origin/mock/<ns>` (`git merge-base --is-ancestor`), so a landing never names a commit that only one machine has. `unborn` stays for a branch not yet created.

8. Migration `directory-to-ref`, in git's terms: refuse on a dirty tree (`git status`); `git subtree split --prefix=<dir> -b mock/<ns>` for the history; push; `git rm -r` the path on dev in one ordinary commit; `data-refs instantiate` at the new path. Edits made on the old path by a feature branch cut before the migration meet dev as modify/delete conflicts, which is what git says for a file the other side removed. A verb `data-refs adopt <branch>` carries them across with `git diff <base>..<branch> -- <old path> | git -C <data worktree> apply --directory=...`, which is git's own apply. A lint refuses files tracked at a retired path after the migration, so the stray copies a merged old branch brings back are named.

9. Interleaved templates. The pattern machinery (`include`, `*`, `**`, the exclude-line negation toggled into sparse lines, `path = "."` with its refusal list, the whole "A ref that holds files by pattern" and "One ref for the whole mock directory" sections of round `202611110830`) exists only because `*.md.tmpl` files sit in the same directory as sources that stay on the branch. That is an artifact of one reader, `src/render_design.rs:577` and the few like it. Move the templates into a tree of their own inside the data worktree, mapped by crate (`templates/<crate dir>/README.md.tmpl`), and have the renderer resolve the template through one function that names the data path. The special case, "a ref holds some files of a directory and leaves the rest to the branch", disappears: one ref is one whole directory is one worktree. This also satisfies op's picture of the branch tree (`design round 202611110830 line 9`: sources plus the generated readme, no template). I have not read every site that reads `*.md.tmpl` (`src/document.rs`, `src/registry/mod.rs`, `src/lint/mod.rs`, `src/entry/nuke.rs`, `src/render_agent/*` all mention it); that count is probe 4 below.

What remains of the engine after this: declaration and its checks, the resolver, `instantiate`, `status`, `verify`, the snapshot, the lint hooks, the pairing. Estimate: the 3400 lines shrink to a few hundred. That figure is an impression; I have not written it.

What an ignorant user can still lose, stated exactly, with probe backing. Edits in a data worktree vanish with `git clean -ffdx` (two f), `git worktree remove --force`, `git reset --hard` or `checkout -f` run inside the data worktree itself, or `rm -rf`. Nothing run in the main worktree reaches the data (p2 control, p6). Everything the engine itself writes is preceded by a snapshot held by a ref. The remaining list is git's own list for any repository, which is the definition of "much like git itself", and it is shorter and stated better than the design round's line 81. It is not zero. I do not believe any design is zero (the snapshot is only taken when an engine verb runs, so an `rm -rf` between two engine commands loses up to that window, same as anywhere), and I will not write a document that claims otherwise.

Merge drivers, LFS, sparse checkout (the user's own, in the main worktree, no longer touched by us), worktrees, bare clones, gc, hooks, signing: each one works because the data worktree is a worktree. Bare clone: probe p4 shows a bare repo with two linked worktrees, a data worktree nested in each, and that the doubled branch is refused by git itself; the variant with a separate local branch per data worktree was not run (probe 9 below). gc: the data branch is a branch; the snapshot is a ref; nothing is reachable only through a file the engine keeps beside a record.

## 4. Where I hold more than one reading

Reading A (section 3): a disjoint path, a worktree on a real branch, the engine shrinks to declaration, snapshot, pairing and verification. What it needs from op: the data path moves, and templates leave their crate directories. If op says the path `mock/research` must stay and the templates must stay beside their crates, A is dead and B is what is left.

Reading B: keep the data at the old path, keep it as a worktree on a real branch (so the 3400 lines still go), and close the forced way back with local `git replace` views of the pre-migration commits (probe p7 shows the mechanism works). The cost is the fragility of regenerating the replacements, a clone-by-clone setup that no remote carries (p7, `refs/replace in clone: 0`), and a window after a fetch of an old branch. Interleaved templates under B need the pattern machinery to stay, or the same relocation as A.

What distinguishes them: whether op will accept a path change (a question for op, not a probe), and the measurement in section 5 probe 3 (how many replace refs a real repository's history needs). If the number is a few hundred, B is workable. If thousands, B is not.

Reading C, the shipped shape corrected: keep A of section 2 and only fix the two red tests and the unimplemented picks. I do not hold this reading. The windows list in round line 81 is a list of git verbs the guards did not cover, and the matrix in section 0.3 shows nobody has tried most of the verbs.

## 5. Probes I could not settle

1. The reftable backend. This machine has git 2.43.0 and no `git refs migrate`. Probe: on git 2.47 or later, run section 3 item 6 (snapshot by engine command, not by hook) under `--ref-format=reftable`, plus `worktree add` of a data branch in a bare reftable clone, and show the ref write and the linked-worktree path both work. Decides whether the whole reftable paragraph of the design (round line 79) is gone.
2. Hooks in a linked data worktree. Does a `pre-commit` installed in the common `hooks/` directory, or under `core.hooksPath`, run for `git commit` inside a nested linked worktree, and does it see `GIT_WORK_TREE`/`GIT_DIR` that identify the data worktree? I believe it does; I did not probe it, so it is an impression.
3. Cost of the disjointness check, and the replace-ref count for reading B. Run `git log --all --name-only --format= -- <path>` and `git rev-list --all -- <path> | wc -l` on mockspace's real history and on the largest repository a consumer has. Decides section 3 item 3 and the B/A choice.
4. The path coupling census for the resolver. For every reader of `*.md.tmpl`, `mock/research`, `research/` and `design_rounds`, mark hardcoded versus config-resolved. The file list is in section 3 item 9. Decides how large the path move is.
5. Windows and case-insensitive filesystems. `git worktree add` nested under an excluded path, `git clean`, and worktree move behave differently with symlinks and long paths. None probed; the engine today refuses symlinks (round line 19); impression only.
6. `git worktree prune` after the user deletes the data directory with `rm -rf`: does the branch survive, does the next `instantiate` recreate cleanly, and does the engine's snapshot ref still hold the last engine-time copy? I believe yes on all three. p6 shows `prune` after `clean -ffdx` leaves only `[dev]`. The recreate and the snapshot were not probed.
7. Old-branch adoption. A pre-migration feature branch that edits `mock/research` files, rebased onto the migrated dev: show the modify/delete conflict, then `adopt`, with real `git apply --directory`. Decides item 8 of section 3.
9. A bare clone, two linked worktrees, each with a nested data worktree on its own tracking branch, edit in one, push, pull in the other, with the section 3 item 2 branch naming.
8. The full verb matrix as one table-driven e2e test, to replace the sampled four at `tests/refs_daily_history.rs:106-111`. Every verb in the list of section 0.3, run in the main worktree after an edit, assert the edit's bytes survive in the data worktree. Under A this is green by construction and cheap; under the shipped design it is the test that finds the next hole.

## 6. The probes committed with this file

All under `mock/research/202611120700_data-ref-architecture/001_probes/`, each a script with its raw output beside it, git 2.43.0:

- `p1_nested_worktree_vs_old_history.sh` / `p1_output.txt`: first nested worktree attempt, before I fixed the data branch to hold only the data. Kept because it is the run that showed plain `checkout OLD` eating the edit and the way back deleting the data worktree's file.
- `p2_collision_matrix.sh` / `p2_output.txt`: hidden, not hidden, and disjoint path against checkout, `checkout -f`, `reset --hard`, `switch --detach`, `stash -u`.
- `p3_gitlink_guard.sh` / `p3_output.txt`: a gitlink at the data path.
- `p4_bare_clone_data_worktrees.sh` / `p4_output.txt`: bare clone, two linked worktrees, one data branch in two places.
- `p5_merge_driver_in_data_worktree.sh` / `p5_output.txt`: custom merge driver from shared config in a data worktree, with a control.
- `p6_disjoint_path_hazards_and_snapshot.sh` / `p6_output.txt`: `clean`, `worktree remove`, `prune`, and the snapshot by temporary index with a clean filter.
- `p7_git_replace_view_of_old_commits.sh` / `p7_output.txt`: replace refs as a view of the old commits.

Limits: each probe is a spike on one git version with one tiny history. They prove that a behaviour exists, not how it prices. The probe scripts' names, `OLD`, `DC` and paths are scaffolding. The p1 script was patched after its first run (the first data commit was wrong); the committed p1 output is of the patched script.

## 7. Dead routes, in order

1. Same-path nested worktree hidden by exclude (p1, p2): loses edits on every old-commit move.
2. Same-path unhidden (p2): plain verbs protected, `-f`/`--hard` destroy.
3. Same-path gitlink (p3): same as 2.
4. One data branch in every linked worktree via `worktree add -f` (p4): stale index, reversal on commit.
5. Separate clone (D): merge drivers, filters and hooks configured in the main repo do not follow (reading from p5, which shows they do follow into a worktree).
6. Submodule (E): forces a dev commit per data change; same `-f` loss.
7. subtree, notes, alternates: wrong shape; the one survivor is `subtree split` for the migration.
8. Replace refs via the remote: fresh clone has none (p7); the hosting limit at `refs.rs:18`.
9. `sparse.expectFilesOutsideOfPatterns`: measured by the shipped test comment at `tests/refs_daily_history.rs:98-100`, not by me.
10. A `post-checkout` or `reference-transaction` guard: runs after the write, or not at all for the `HEAD` symref move (same comment, not independently probed).

I found no way to make `checkout -f OLD` safe at the old path without either the replace-ref view or moving the path. That is the wall, stated as exactly as I can. Help that would move it: someone who knows git's unpack-trees code and can say whether there is a configuration under which an excluded path that is also tracked in the target is never written; I could not find one in the documentation and did not read the source.

## 8. The shipped tree reported, as asked

Unlicensed mechanisms noticed beyond the brief:

- A second remote form, `RemoteForm::Ref`, with no caller that can reach a proxy that refuses it (`refs.rs:46-62`, `refs.rs:18`).
- `commit-tree` plus `update-ref` for every commit: no hooks, no signing, no filters (`USAGE_GUIDE.md:108`, `files.rs:228`, `backup.rs:320`, `ref_ops.rs:303`).
- A merge hook trait that the shipped `update` instantiates with `Refuse` (`src/data_refs/verbs.rs:317`): a merge point built, then wired to refuse.
- An unreferenced-object guard (`pending.rs`) that op ruled unsound on 12 November and is still shipped, plus a user cache op asked for that does not exist.
- A test that enforces the refused LFS option (section 0.2) and a reftable test that returns green without running (section 0.1).

I spent effort on what is wrong with this tree and I will say what it cost: a design round of 87 dense lines, about 3400 lines of reimplemented git in the named paths, and two red tests plus a green test that defends the wrong answer, all to avoid giving the data a worktree. Op's question at `000_op.md:51` was the right question and it was answered correctly by the first person to ask what git does. Nobody asked it before the engine was written.

## 9. Suite result

Run: `CARGO_INCREMENTAL=0 cargo test --workspace --no-fail-fast`, git 2.43.0, other seats running the same suite on the same machine at the same time (so timing-sensitive results are noisy). Raw output: `001_probes/suite_run_output.txt`. Totals summed from the `test result` lines: 3569 passed, 5 failed, 87 ignored. Brief said two red tests. There are five:

1. `tests/refs_daily_history.rs:105` `an_edit_taken_to_an_old_commit_by_checkout_...` (the brief's first). Red at `:131`.
2. `tests/refs_consumer_setups.rs:168` `every_migration_keeps_a_bare_clone_with_linked_worktrees` (the brief's second). Red at `:112`, the assert on the refused `fetch dev:dev`, `branch -f dev`, `checkout dev` from a linked worktree.
3. `tests/refs_consumer_setups.rs:163` `every_migration_keeps_git_shook_dispatching_the_hooks`. Red at `:24`: "git-shook is not on the PATH". A test that fails because a tool is not installed, not because the engine is wrong. Either it is a real requirement and the suite is red on any clean machine, or it should be ignored with a reason. Left red, it teaches everybody that red in this file is noise, which is how the two real reds get ignored. The brief did not list it.
4. `tests/refs_cost_scaling.rs:160` `a_command_on_one_edited_file_costs_the_same_on_a_tree_five_times_larger`: `left: 10394, right: 10397`, "commit of the ref by pattern, one template edited: what git is handed grows with the tree". A three byte difference on an exact equality. I did not open the test to find what varies by three bytes (path lengths of a temp directory is my guess, an impression). Same output shows what a one-file commit costs the engine: `commit, one file edited: spawns 148`, and `commit of the ref by pattern ... spawns 134`, against one `git commit` natively. That is the price of reimplementing add, and it is evidence for the proposal, not just noise.
5. `tests/bench_generated_e2e.rs:164` `a_config_only_nested_tree_runs_end_to_end_through_the_generated_driver`: "missing promoted artifact hash_n64.csv". Outside the data ref surface. Not investigated. The container may be the cause; I do not know.

Two of the five reds are known and honest. Three more were not in the brief, and one of those three is the engine telling its own test suite it does not scale exactly the way its test says.
