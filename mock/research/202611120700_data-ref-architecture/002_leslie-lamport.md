# Data ref architecture, seat 002 (Leslie Lamport)

Gates. Canon gate: mockspace v2 declares no `canon_paths`, so there is no ratified canon. What governs is op's intent in `000_op.md:69` ("no data loss anywhere, ever; machinery that makes losing data impossible even for someone who does not know what they are doing, the way git itself does; a recovery cache under every step that overwrites, changes or deletes files, without leaning on it; and behaviour that stays as close as possible to git's own, so that git's tooling, merge drivers and LFS work unchanged"). The brief asks a question that intent poses, so the gate passes. I am the first reader of the intent in the sense of this seat only; no second-read agreement exists yet and none is claimed.

Test gate. I did not run the whole suite. The saturation guard refused cargo for several minutes (other cargo processes were running), and the full `cargo test` of this workspace is larger than this dispatch could carry. I ran two files, `refs_daily_history` and `refs_consumer_setups`, and started a batch of all `tests/refs_*.rs`; its result is at the end of this file under "Suite". So the gate is only partly discharged, and I say so rather than claim green. What the partial run measured is under finding 1.4. I read the oracle all the refs scenarios share (`Ledger::kept`, `tests/refs_e2e/whole.rs:368-383`): it asserts that each written text is on a disk at its place, or its blob is reachable from some ref of a clone or of the remote. That is a real oracle, not a tautology. Its weakness is the matrix it is fed, which is a sampled law (finding 1.4).

Evidence rule. Every probe cited here is committed in `mock/research/202611120700_data-ref-architecture/002_probes/` with its output beside it, `p1` to `p6`. They are git 2.43.0 spikes of one thing each. They prove existence and refute universals; they price nothing. Where I rely on something I did not probe I say "impression".

## 1. The brief, broken

### 1.1 Premises that are false or conflated

a. "a git ref of its own (`refs/heads/mock/<namespace>`)". There are two forms. `form` is `branch` (default, `refs/heads/mock/<ns>`) or `ref` (`refs/mock/<ns>`), `src/data_refs.rs:29-30` and `:39-41`. A third ref family, `refs/mock-backup/<ns>`, local and never pushed, holds the engine's copies, `mock/crates/mockspace-store/src/instance/backup.rs:5-6`. Any alternative that needs a branch (a git worktree needs one, or a detached head) cannot take `form = "ref"` as it stands.

b. "(`research`, `templates`, registry rows and the like)". Registry rows are not on a data ref. The data-ref declaration refuses the registry, the flat open round and the test history as a `path` (`src/data_refs.rs:67-68`, `OWN_STORES`) and refuses a namespace a registry store already uses (`:83-99`). The registry's refs are its own stores (`mock/crates/mockspace-registry/src/test_world.rs:44`, `refs/heads/mock/registry/ticket`). The two share the `RefStore` type and nothing in their directories' semantics. A panel that treats "the shared directory" as one thing will judge the registry by the data-ref engine's problems and the reverse.

c. "materialises it at paths inside the main worktree". That is the default only. `instance` places it elsewhere, relative to the mock directory (`src/data_refs.rs:35-37`), and the design round states an instance may lie outside every work tree (`202611110830_topic...:59`, last sentence). The engine already has the one degree of freedom that the best alternative below needs, and the brief hides it.

d. The brief names the shipped surface as "the engine" and "the migration". What the directories around the named paths add: `src/data_refs.rs` (the declaration and its validation, 140+ lines, the root of the module), `src/bootstrap/hooks.rs` (the pre-push stand-down, `:1229-1244`), the `Mock-Ref` pairing (`src/pairing`, design round `:33-35`), `mock/crates/mockspace-registry/src/stores.rs:294-298` (`RefDecl`), and in the store crate `ref_store.rs`, `merge.rs`, `commit_reader.rs`, `attributes.rs`, `git.rs`, `retry.rs`. In the instance directory the brief's "and the rest" is `files.rs`, `held.rs`, `lines.rs`, `stats.rs`, `tracking.rs`. The brief's own named directories total 4964 lines; the instance directory alone is 1919 lines (`wc -l`), and that is a hand-built index (stat stamps, racy-timestamp rule: design round `:65`), ignore engine (`:59`), filter comparison (`:73`), executable-bit logic (`:63`), commit and three-way apply (`apply.rs`). That is the drift, measured in lines, before a single test.

e. "Two red tests pin losses that are still open." At least three are red in my run: the two named, plus `every_migration_keeps_git_shook_dispatching_the_hooks` (`tests/refs_consumer_setups.rs:24`, panics "git-shook is not on the PATH"). That third one is environmental, not a loss, but a test that cannot run without a tool the repository does not provide is red by construction in every clone without it, and nobody reading "two red" would know.

f. The brief and `000_op.md:35,59` speak of a "ref-held pending copy" and a "user cache" as picked. Neither exists in the source. `grep` for a user cache under `src/data_refs` and the store's `instance/` finds none (the `cache_dir` hits are in `src/custom_lints/store`, unrelated). The pending copy is still objects named by a file, pruned by `git gc` after `gc.pruneExpire` (`mock/crates/mockspace-store/src/instance/pending.rs:17-20`, design round `:79`). Op's picks are decisions, not shipped state; any claim that the day-long cache "sits under every overwriting step" is false today.

g. `expectFilesOutsideOfPatterns` was picked (`000_op.md:19-21`, option c) and is set nowhere in source (grep over `*.rs` and `*.md`: only `000_op.md` matches). And the design round `:71` still says "per-worktree config is turned on for the block", `src/data_refs/sparse.rs:121-123,198-215`, which is exactly the mechanism op's lean ("sparse shared") removes. The first design round is therefore stale against op's own words in three places. It is a subject, not authority.

### 1.2 What the engine does not do that the brief assumes it does

`update` does not use git's merge. A file changed both on disk and on the tip is a refusal: `mock/crates/mockspace-store/src/instance/apply.rs:174-188` ("holds files changed here and on the tip"). Merge drivers and `.gitattributes` are never consulted for data files. That confirms op's suspicion (`000_op.md:51`) at the level of source.

### 1.3 The hook cannot be what makes loss impossible

The guard depends on a `reference-transaction` hook that "never stops the person's git command: it exits 0 whatever the launcher does" (design round `:77`), is "never written over a hook of the person's" (same line), and is skipped by every `git -c core.hooksPath=...` or by a hooks directory the person owns. `every_migration_keeps_a_reference_transaction_hook_of_the_persons_own` passes: the person's hook is kept, hence the guard is absent, and `verify` merely names it. A mechanism that is best-effort by design cannot be what "makes losing data impossible". It can be what makes it less likely. Op's intent distinguishes the two ("without leaning on it").

### 1.4 The tests: sampled laws, and a matrix that omits the loser

`an_edit_taken_to_an_old_commit_by_checkout_with_no_engine_command_since_is_kept_on_the_forced_way_back` (`tests/refs_daily_history.rs:105-132`) is red, correctly and on purpose; I reproduced it (all four ways lose the edit). Good. But its matrix is four ways: `{checkout, checkout -f}` to the old commit, then `{checkout -f dev, reset --hard dev}` back, `:106-111`. Every way back is a forced one. Probe p2 ran six shapes, adding `reset --hard OLD` to start and a plain `checkout dev` to end. Result: with `sparse.expectFilesOutsideOfPatterns=true` (op's pick c), four of the six keep the edit and `checkout OLD ; checkout dev` (plain, unforced, the most ordinary two commands in git) still deletes it, `002_probes/p2_output.txt`. The test never names the shape that pick c leaves open, and `reset --hard OLD` is absent from the red test (it is in a green one only through the cherry-pick scenario). A fundamental law ("an edit is never lost across a trip to an old commit") tested at four of the shapes git has (add `merge`, `rebase`, `switch`, `restore`, `stash`, `worktree remove`, `clean`) is a sampled law and the suite is not complete on this property. It is the honest kind of incomplete, red where it is red, but whoever wrote the matrix chose which forced ways to include, and the unforced loser was left out. Do not read the eventual green of that test as the property.

The second named red test, `every_migration_keeps_a_bare_clone_with_linked_worktrees` (`tests/refs_consumer_setups.rs:168`, body `:100-112`), is red with "refusing to fetch into branch 'refs/heads/dev' checked out at '.../a.git'". The cause is in source: `worktree_config` moves `core.bare` from the shared config into `config.worktree` of the main worktree (`src/data_refs/sparse.rs:205-213`), which makes the bare directory read as a worktree that has `dev` checked out. The engine edits the one setting every git tool reads first, in order to host a feature that exists only because the instance lives in the main tree.

## 2. Derivation

### 2.1 The need, as a state machine

State of one clone: the tip T of the data branch, the set F of files materialised for people and tools to read and edit, and the base B they were made from. Two sub-problems are separable and the shipped engine fuses them.

Q, the history problem: commit F onto the branch, move T from and to the remote, combine two concurrent edits. Everything git already does for a branch.

P, the placement problem: F must be readable at a path the tools use, and no command of the main branch may write over F. This arises only because some old commit of the main branch tracks the path (the pre-migration tree), and because F is hidden from the branch by exclusion, which git treats as expendable (`sparse.rs:3-8`).

Invariant I1 (the one op wants): every byte a person wrote into F is, at every instant, in a file at a path a person can name, or in an object reachable from a ref, or in the cache. The loss paths are exactly the steps that falsify I1.

The shipped design solves Q by re-implementing it (own index, own merge refusal, own commit and push, 1919 lines of instance) and P by exclusion, a sparse checkout, a hook and a copy. The question is which of these is native git.

### 2.2 Candidates, by category

A. The current shape: instance files excluded, sparse checkout skip-worktree, `reference-transaction` hook, three-set backup ref, own sync verbs.

B. Native linked worktree of the data branch, nested at the same path inside the main worktree (op's own example, a "full parallel branch path").

C. Native linked worktree of the data branch located outside the main worktree (under the common git directory), reached from the main worktree by an untracked, excluded symlink to the directory.

D. Submodule: a gitlink at the data path of each branch commit.

E. `git subtree`: the data is in the branch's history.

F. A second repository (sibling clone, own remote), optionally sharing objects through alternates.

G. `git notes` or `git replace` as the carrier.

H. Sparse checkout of the main branch alone, no data ref (data stays on `dev`, hidden).

### 2.3 What each does, with the scenarios that break it

Probes first, because they end arguments.

p1 and p3 (`002_probes/p1_output.txt`, `p3_output.txt`): B, the nested native worktree, protects against nothing the current shape does not. The main worktree's `checkout OLD` writes the old tracked bytes into the nested worktree's directory: `N.md` was `EDIT-A`, became `old`; after the way back the file was gone and the nested worktree reported ` D N.md`. Adding the sparse checkout and `expectFilesOutsideOfPatterns` to B reproduces p2 row for row (p3: off, every row lost; on, four kept, plain checkout-then-checkout lost). So B changes who owns the files and the merge, not who can destroy them. Its other measured costs: one branch in two worktrees is refused (`p1_output.txt`, "'mock/research' is already used by worktree at .../main/mock/research"), so a linked worktree of the main repo cannot have its own data worktree on the same branch; and `git clean -ffdx` in the main tree deletes the nested worktree's uncommitted edit and leaves the worktree record "prunable" (`p2_output.txt`, last block). One-f clean leaves it alone.

p4 (`002_probes/p4_output.txt`): C survives all six ways to an old commit and back, and `clean -fdx` and `clean -ffdx`. The edit sits in `.git/mock-data/research/N.md`, a path no command of the main branch addresses. What the old checkout and the clean destroy is the symlink, which holds no data. The sixth way (`reset --hard OLD`) leaves a real directory where the link was, holding the old tracked bytes; the edit is still safe at the real path. No sparse checkout, no hook, no per-worktree config, no `core.bare` move are involved. That is op's intent ("impossible, much like git itself") obtained by placement, with nothing leaning on a hook that can be absent.

p5 (`002_probes/p5_native_merge_gc_bare.sh`, `p5_output.txt`): in C, a bare clone with two linked worktrees and the data worktree under the bare directory: both linked worktrees see the same data through their link; no `extensions.worktreeConfig`, no sparse (`config --list | grep` count 0); a dirty, uncommitted data edit survives `git gc --prune=now --aggressive`; a second clone's concurrent edit to the same file is merged by plain `git pull` in the data worktree through a custom merge driver named in the data branch's own `.gitattributes` (`a b x y`, the driver's output; default merge would have conflicted). That is op's merge-driver request met with zero engine code. Note the probe's link was not excluded, hence `?? research-link` in its status; excluding it is one line in `info/exclude`. In a bare clone the data branch already arrives as an ordinary local branch (the probe needed no fetch refspec).

p6 (`002_probes/p6_output.txt`): in the current shape, other ordinary commands delete an edited, hidden file with no refusal: `git worktree remove` of a linked worktree (rc 0, directory gone, edit gone, no warning), `git clean -fdX`, and `git stash -a` takes the file out of the tree (it is in the stash, so recoverable, but the next engine look sees "no files on disk", design round `:31`). The shipped matrix has none of `worktree remove` and `clean -fdX`. In C the hidden thing is only a symlink, so these commands lose nothing.

D, submodule (impression, unprobed). A gitlink at the data path pins the data commit per branch commit. It gives Q natively and gives pairing for free (it replaces the `Mock-Ref` trailer, design round `:35`). It costs: every data commit forces a branch commit to move the pointer, so a data edit is no longer outside the branch; two clones moving the pointer concurrently is a merge conflict on every branch merge; and old commits track the path as a tree, not a gitlink, so the main checkout would refuse or write across the type change. That is the same old-commit problem plus a new conflict class. Rejected as the carrier; the pairing idea is the useful residue.

E, subtree (impression). The data is in the branch history. This contradicts the premise the whole design sits on (templates leave the tree and history of the branch). Rejected.

F, second repository (impression plus the p5 mechanics). Fully vanilla git for the data: its own hooks, LFS, merge drivers, gc, its own pairing by cross-repo id trailers. Costs a second remote per project and a second clone to keep in step; hosts that offer one repository per project make that a real cost. Equal to C in data safety (the data is out of reach of main-tree commands) and not better. Keep as the answer if a host disallows extra branches; labelled taste and a hosting fact, not mathematics.

G, notes or replace (impression). Notes attach a blob to a commit under `refs/notes/*` and need a merge strategy of their own; they carry a message per commit, not a directory tree of editable files. `git replace` rewrites an object's meaning locally and does not travel by default. Neither is a store. Notes are conceivable as the carrier of the pairing record instead of a trailer; the trailer is immutable and travels with the commit, which is stronger. Rejected.

H, no ref. Contradicts the intent that the branch tree holds only crate sources and the generated readme (design round `:9-11`). Rejected as a different system.

### 2.4 What each does to the data-loss paths

Loss paths of A, with their citations (all but L1 to L3 are by reading, not by run):

L1 forced and unforced way back from an old commit: red test, p2 (`002_probes/p2_output.txt`). Pick c closes four of six, leaves plain `checkout OLD; checkout dev`.

L2 `git clean -fdx`/`-fdX`: the instance vanishes (`p2_output.txt`, "after clean -fdx: ... No such file or directory"; design round `:31` admits the record stays and `restore` writes files from the commit the instance was at, which is no help for the edit made since).

L3 `git worktree remove` of a linked worktree: p6.

L4 window before any copy exists: design round `:81` lists three shapes the engine itself pins.

L5 pending copy pruned by `git gc` (`pending.rs:17-20`; design round `:79`). Op's picked remedy is not built (finding 1.1 f).

L6 the guard is best-effort by design (finding 1.3). Anything that bypasses or displaces the hook turns every other path from guarded to open.

L7 "was it git or the person who wrote this?" is inferred from bytes: a file the object database already holds is read as git's (design round `:73,:75`, "One that puts back bytes a commit or an earlier copy holds is taken for git's too"). An inference from content to intent, with a stated false positive. It gates `commit` on a guess.

L8 the engine edits the person's own git config: cone-mode sparse is switched to full-pattern mode (`sparse.rs:124-131`), `core.bare` and `core.worktree` are moved (`:205-213`). The second of these is the cause of the second red test.

L9 stat-stamp shortcut (design round `:65`) trusts a clock and a filesystem; stated by the round itself.

L10 backup growth: one commit per distinct state, never pushed, no expiry stated. Not a loss; a different cost.

Loss paths of B: all of L1, L2 (single-f clean survives, -ff does not), L4 (a native worktree has no copy before any engine command, same window), plus the one-branch-one-worktree refusal for linked worktrees of the main repo. Gains: native merge, native LFS, native commit in the data directory.

Loss paths of C: the data directory is touched only by commands run on it. Remaining paths: (i) an explicit `git reset --hard`, `git checkout -f` or `git clean -fdx` inside the data worktree, which is git's own documented destroyer and for which no tool, including git, offers a net; the cache below is that net. (ii) `rm -rf` of the clone or the bare directory, which loses every unpushed branch in git too. (iii) moving the clone directory leaves the worktree record pointing at the old absolute path; `git worktree repair` is the native fix (impression: not probed). (iv) the symlink itself: a file-level link is unsafe, since any editor or `sed -i` that saves by rename replaces the link with a regular file and the edit leaves the data tree. A directory link is safe because a rename inside it resolves through the link (impression: standard POSIX, not probed). So C is for whole directories only.

### 2.5 Who can break what, for someone who does not know the machinery

A. Anyone who runs `git clean -fdX`, `git worktree remove`, a checkout of an old branch, or has their own `reference-transaction` or `core.hooksPath`. Four routine commands, none of which looks dangerous.

B. The same people, for the same commands (p1, p3), and now also `git clean -ff`.

C. Someone who runs `git reset --hard` or `git clean -fdx` inside the data directory, which is a deliberate destroy in git. A person who runs routine commands in the main tree cannot reach the data (p4). That is the property "much like git itself": the destructive commands are the ones git has always made destructive, and the safe ones stay safe.

### 2.6 The one real obstruction to C

Directory symlinks cannot interleave. The design round's reason for patterns is that templates "sit in the same directories as the sources that stay" (`202611110830_topic...:11`), and the ticket stamps `path = "."` with patterns that take every `*.md.tmpl` and `catalogue.md` out of crate directories (`tickets-mockspace.toml:1500`). A symlink for `mock/crates/<crate>/` would hide the sources; per-file links fail as in 2.4(iv). So C covers every data directory that is a directory of its own (research, benches, tools, lints, closed design rounds, test history if moved) and does not cover the interleaved templates. That is the entire reason the shipped stack (sparse lines, exclusion lines, patterns, held files, hook) exists. It is a fork about layout, and it is op's, not mine: relocate the templates into the data tree in a mirrored structure (`<data>/design/<crate path>/x.md.tmpl`), or keep interleaving and keep the whole stack. I take the first as the build and the second as the stated fallback in section 3.

## 3. Proposal

I would build C for directory-shaped data, with a recovery cache that is a plain git object store, and I would stop supporting patterned interleave in the engine unless op says the layout cannot move.

### 3.1 Storage and materialisation

1. One data branch per declared namespace, `refs/heads/mock/<ns>`. Drop `form = "ref"` for data refs (a worktree needs a branch or a detached head; registry stores keep both forms). One definition of a namespace stays in the store's check (design round `:35`).
2. One native worktree per clone for all data branches, located under the common git directory, for example `<git-common-dir>/mock-data/<ns>`. Created by `git worktree add` (`--orphan` where the branch is unborn, git 2.42 and later; older git: detached add then `checkout --orphan`). Taste, labelled: the location under the common dir versus a sibling directory next to the clone. Nothing proved distinguishes them.
3. In every worktree of the repository (main and linked), `mock/<dir>` is an untracked directory symlink to the data worktree's `<dir>`, excluded in the common `info/exclude`. One data worktree serves every linked worktree, matching "shared directory of a repository". No per-worktree config, no sparse checkout, no `core.bare` move. The second red test cannot occur because nothing the test measures is touched (p5: zero worktree-config or sparse keys in a bare clone with two linked worktrees).
4. `data-refs instantiate` means: create the worktree if absent, ensure each link, and if a link's place holds a real directory (git wrote the old tracked bytes there, p4 sixth row), rename it aside under a stamped name and never delete it, then link. Idempotent and safe to repeat.
5. The data worktree is a vanilla worktree. `git -C <it> status|diff|add|commit|pull|merge|lfs` work with nothing between them and the engine. `update` is `git pull` (or fetch plus merge) in it. Merge drivers named in the data branch's `.gitattributes` and defined in anyone's git config apply (p5). The refusal at `apply.rs:174-188` has no counterpart: a conflict is git's conflict, markers in the file, nothing lost.
6. The engine verbs become conveniences over that: `commit` runs the repo's lints first (design round `:45`) then `git add -A && git commit` in the data worktree and pushes; the pre-push stand-down (`:47`) becomes a check of the pushed ref names, unchanged in kind.

### 3.2 The recovery cache, as a git object store

Op's intent is a cache under every step that overwrites, without leaning on it. Under C the overwriting steps are few and all inside the engine or inside a command run in the data worktree.

7. Cache snapshot: build a tree of the whole data worktree through a temporary index (`GIT_INDEX_FILE=<tmp> git add -A && git write-tree && git commit-tree`) and hold it at `refs/mock-cache/<ns>/<n>`. Ref-held, so `git gc` cannot prune it (this is the op-picked "ref-held" requirement, and it replaces the `pending.rs` mechanism and its two-week caveat). One mechanism replaces the three-set `edit`/`guard`/`lost` format of `backup.rs`.
8. Taken by the engine immediately before each engine step that overwrites: `update`, `restore`, `instantiate` relink, any migration step, and on every engine invocation that looks at the data. A user cache directory, outside the repository, receives the same snapshot as a plain copy and is pruned after a day, as op specified; it is the only layer that survives deleting the clone. Ref plus directory is exactly op's "Ref-held + cache" (`000_op.md:35`).
9. Nothing in the safety of the main-tree commands depends on the snapshot, which is the "without leaning on it" clause: p4 shows those commands cannot reach the data. The snapshot guards the one class C does not make impossible, an explicit destroying command run inside the data worktree. Optional, impression: a `reference-transaction` hook confined to the data worktree's own ref moves (`reset`) can take the snapshot before git moves HEAD; it is best-effort for the reason in 1.3 and I would not count on it.

### 3.3 The migration, one way

10. Per step, snapshot first (cache ref and user directory), then act, then verify byte equality, then remove the original last, like `git mv`: (a) snapshot the files to move; (b) make the orphan data branch and commit them; (c) `git worktree add`; (d) `git rm --cached` of the tracked paths on `dev` and a commit; (e) remove the now-excluded old files only after a byte comparison against the data branch; (f) link. A run stopped part way is finished by the same call, as the design round already requires (`:51`). `migrate_refs.rs` and `migrate.rs` are 1013 lines; I did not read them and make no claim about how close they come (impression).

### 3.4 What is deleted

`src/data_refs/sparse.rs` (216 lines), `src/data_refs/hook.rs` (174), `instance/backup.rs`, `pending.rs`, `overwritten.rs`, `held.rs`, `lines.rs`, `stats.rs`, `tracking.rs`, `walk.rs`, `files.rs`, `apply.rs` and the record machinery, the stat-stamp index, the ignore re-implementation, the filter comparison and the executable-bit logic. They exist to make a non-git directory behave like a worktree. The design round `:59-65,:73` already conceded how much is being reproduced; the proposal stops reproducing it. Per the workspace rule that nothing appears in code that is not in the design, and a design change nukes the code under it, this is a design change: it nukes the data-ref code and its tests, and the tests that pin loss shapes become tests of C (below).

### 3.5 Readings held, and what separates them

Reading A (the one I would build): C as above, with templates relocated into the data tree.
Reading B: keep the shipped stack and harden it: pick c (`expectFilesOutsideOfPatterns`), shared sparse (no per-worktree config), the ref-held pending copy, the user cache. Right if and only if the templates cannot leave the crate directories. It still has L1 for plain checkout (p2), L2, L3 and L6 open; it would have to close those by new mechanism each.
Reading C: nested native worktree (op's example) with sparse. Its only gain over B is native merge, LFS and commit (p3 loses exactly as B). It is the answer if symlinks are unacceptable on a target platform and interleave is not needed. Windows symlink behaviour is not something I opened (impression).
Reading D: a second repository. Right if a host will not carry the extra branches.

What distinguishes A from B is one measurable fact about the layout: whether anything in the loaders depends on a template's being beside its crate's sources. That is a question about `docs/REFERENCE-SYNTAX.md` and the template discovery, not about git.

### 3.6 Tests that this proposal owes

Each as a contract, and the first of them is meant not to compile or to fail until built.

- The six-shape matrix of p2 and p4, extended with `merge`, `rebase`, `switch`, `restore`, `stash -u`/`-a`, `worktree remove`, `clean -fdX`, `clean -ffdx`, each with an edit in the data directory, asserting the bytes are at the real path. The shipped matrix is a sampled law.
- Merge driver applied on `update` (p5 as a test), LFS pointer round trip in the data worktree, bare clone with two linked worktrees and `fetch dev:dev` plus `branch -f dev` plus `checkout dev` from a linked worktree (the red bare test, which must go green with no engine code touching `core.bare`).
- Relink after `reset --hard OLD` (p4 row 6): the directory is renamed aside, never deleted.
- Snapshot ref survives `git gc --prune=now`.

## 4. Probes that would settle what I could not

1. Template layout: do the loaders, `docs/REFERENCE-SYNTAX.md` resolution and `references-to-letkajenkka` tolerate templates in a mirrored tree under the data directory? Settles A versus B.
2. LFS in the data worktree: `git lfs` hooks live in the common hooks directory; probe a pointer file edited, committed and pushed from the data worktree under `core.hooksPath` set to a durable directory and to git-shook. git-lfs 3.4.1 is installed here; I did not run it.
3. `git worktree repair` after moving the clone directory, with the worktree under the common git directory, and after `git clone --local`.
4. Directory-symlink behaviour under the tools that matter: cargo's directory walker, the engine's own symlink refusals (design round `:19` refuses writes below a symlink; that refusal inverts under C), editors that save by rename, `rg`, and a Windows junction or symlink.
5. Commit hooks: a `pre-commit` hook of the branch runs for a commit in the data worktree too (`core.hooksPath` is common). Probe the discrimination by branch name, and the cost of running the repository's lints on every data commit.
6. Submodule (D) against an old commit that tracks the path as a tree: refuse, overwrite, or keep. I rejected D on an unprobed impression.
7. Reftable backend: confirm that under C no hook has to write a ref inside another ref's transaction, since the design round's reftable lock analysis (`:79`) comes entirely from the hook that C deletes.
8. Interleaved patterns under C, if op wants them: whether some arrangement of directory links plus `include` can carry templates among sources without a per-file link. I found none.

## Suite

Partial, as stated at the top. Two files: `refs_daily_history` 8 passed, 1 failed (the named red); `refs_consumer_setups` 3 passed, 2 failed (the named bare-clone red, and the git-shook test, red because `git-shook` is not on PATH). The batch of every `tests/refs_*.rs` had not finished when I committed (`refs_consumer_interrupted` was still running its kill-at-each-git-call scenarios, each over 60 seconds); before that point `refs_backup` passed 14 of 14 and `refs_checkout` 7 of 7. The remaining files are unmeasured by me.
