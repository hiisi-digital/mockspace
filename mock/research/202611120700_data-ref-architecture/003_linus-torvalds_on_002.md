# Seat 001 on seat 002

Read: `002_leslie-lamport.md` (195 lines) and its `002_probes/p4_output.txt`. Probes for this file are in `003_probes/` (q1 to q4, scripts with raw output beside them), git 2.43.0, each a spike of one thing, none a bench.

Order of business: where 002 is right and my file is wrong, then where 002 is wrong, then the proposal that stands after both attacks.

## 1. Where 002 is right and 001 is wrong

1. My proposal has a silent loss path that I wrote past and 002 had the instinct for. 001 section 3 nests the data worktree inside each main worktree, hidden by `info/exclude`, and calls `git worktree remove` protected ("refuses if dirty", p6 of mine tested only `clean`). Probe q1 (`003_probes/q1_output.txt`): a linked worktree `w2` holding a nested data worktree with an uncommitted edit, `git worktree remove w2` with no force: no refusal, `parent exists=no`. The nested directory is ignored content of its parent and git deletes ignored content. That is the same class 002 found in the current shape (002 finding 1.4 / L3, p6), and it is routine in exactly the workflow this repository runs (this panel's seats are linked worktrees under `.worktrees/`). My text said the remaining loss list was "git's own list for any repository". For a linked worktree it is not: it includes the most ordinary cleanup command there is. Wrong, and the 001 file should be read with that sentence struck.

2. A data worktree under the common git directory, one per repository, is immune to everything run in a main worktree, including that command. 002's location (`<git-common-dir>/mock-data/<ns>`, 002 section 3.1 item 2) is not "taste" as 002 labels it; q1 is the measurement that distinguishes it from my nested placement. 002 is right and I was wrong to put the data inside something git considers disposable.

3. One data worktree for all linked worktrees matches the phrase "shared directory of a repository" and avoids what my per-worktree-branch scheme (001 section 3 item 2) creates: N local branches, N snapshot refs, N divergent copies of one research directory that git then has to merge. I gave that cost no weight. 002's reading is the simpler data structure.

4. 002 finding 1.3 is right and I missed it: the guard is a hook the person's own `reference-transaction` hook displaces (`src/data_refs/hook.rs:96-99`: "is a hook of your own, and the copy ... needs a `reference-transaction` hook: have its `prepared` case run `mock data-refs snapshot`"). So even where the engine's design works, a person who already has such a hook has no guard, and a green test (`every_migration_keeps_a_reference_transaction_hook_of_the_persons_own`) certifies that state. I wrote only that the hook runs after the write.

5. 002 finding 1.1b and c are premise corrections I did not make: registry rows are not on data refs (`src/data_refs.rs:67-68`, `OWN_STORES`), and `instance` already places the directory elsewhere (`src/data_refs.rs:35-37`: "Where the instance is placed, relative to the mock directory"). My claim that a path move needs "a resolver change" ignored that the placement degree of freedom is shipped. I should have found it.

6. 002 adds a layer I omitted: a user cache directory outside the repository is the only layer that survives deleting the clone (002 section 3.2 item 8). Correct.

7. 002 states plainly that it did not run the whole suite (its "Test gate" paragraph). That is an honest gap against the standing gate, and I ran the whole thing, so I can supply the number it lacked: five red, not three (my file section 9: the two named, git-shook, `refs_cost_scaling.rs:160`, `bench_generated_e2e.rs:164`). Its "at least three" is true and incomplete.

## 2. Where 002 is wrong or unsupported, with the replacement

### 2.1 The symlink is refuted by this repository's own data (the main attack)

002's proposal C makes `mock/<dir>` an untracked directory symlink to the data worktree under `.git/` (002 section 3.1 item 3), and its probe p4 tests the link with one file, `N.md`. p4 never puts a relative path in the data that points back into the main tree. Probe q2 does, and the repository does it: 13 of the 24 `Cargo.toml` under this repository's `mock/research` carry a path dependency that climbs to the repo root (`mockspace-bench-harness = { path = "../../../../bench-harness" }` appears 11 times, `bench-core` with five `..`, others `../support`, `../shared`; counted with grep over `mock/research`).

q2 (a1, a2), with a sketch at `<data>/sketch` depending on `../../../crates/lib` through the link:

- the shell: `cd mock/research/sketch && cat ../../../crates/lib/Cargo.toml` fails, since `..` is resolved physically, under `.git/mock-data`;
- cargo: `cargo metadata` fails with "unable to update .../.git/crates/lib": it resolved the path dependency physically, into `.git/`.

So under C, every research probe that depends on a workspace crate stops building. That is not an exotic tool; it is cargo, in a repository whose research directory exists to hold cargo spikes (`src/deny.rs:175-195` names exactly that). 002 listed "directory-symlink behaviour under cargo's directory walker" as an owed probe (its section 4 item 4) and then proposed building on the symlink anyway. The probe was owed before the proposal, and it comes back red.

What stands in its place: not a symlink. The files must be physically inside the tree at the depth the data assumes. That is my nested placement (a real directory) with q1's hole repaired (2.2 below), or a symlink only for directories whose content never leaves itself, which in this repository is not research.

Correction owed to my own file by the same fact: my disjoint path (001 section 3 item 3, example `mock/data/research`) is one directory deeper than `mock/research`, which breaks the same 13 relative paths. The disjoint path must have the same depth as the path it replaces. 001 understated this.

### 2.2 Repairing nested placement: do not hide it

q4 (`003_probes/q4_output.txt`): the same nested data worktree in a linked worktree, but not excluded.

- `git worktree remove <parent>` from the main worktree refuses: "contains modified or untracked files, use --force to delete it". The vanilla guard now applies, because the data is untracked content, not ignored content.
- `git stash -u` prints "Ignoring path"; `git clean -fd` prints "Skipping repository". Edit survives both.
- cost: `git status` shows `?? mock/`, and `git add -A` adds the nested repository as a gitlink and `git commit` records `160000 commit ... mock/data/research` on the branch, with only a warning. That is pollution, not loss. A pre-commit check refusing any mode-160000 entry at a declared data path closes it, and `verify` can name one already committed.

Replacement: the data worktree is nested and not excluded; hiding is what made it expendable (my own p2 had shown the same of plain checkout, and I drew the wrong conclusion from it by reaching for exclude for the sake of status noise). This trades status noise for loss, and loss is the thing op ordered against. `cargo mock status` can print a data path as data. One line in the guide covers `git add -A`.

Residue that stays: `git worktree remove --force`, `git clean -ffdx`, `rm -rf`: the user said it twice (q1 last line, 001 p6).

### 2.3 "C survives all six" is a sampled law over six, and the surviving thing is not the whole story

002 p4 ran six ways and two cleans. I ran eight verbs against the symlinked shape, each in a fresh repository (`q2_output.txt`, b1 to b8: path-form `checkout OLD -- path`, `restore --source`, `merge` of a pre-migration branch, whole-tree `checkout`, `checkout -f`, `reset --hard`, `stash -u`, `cherry-pick`). The data edit survives all eight. That confirms 002's central claim over a wider matrix, and I say so plainly.

What the matrix also shows, and 002 states only for one row: in seven of eight the link is destroyed and replaced by a real directory of tracked files (`link still a link=NO`). In the `merge` and `cherry-pick` rows it is a directory holding a modify/delete conflict ("Version oldbranch of mock/research/NOTES.md left in tree"), mid-operation. The person is now looking at a `mock/research` that is not the research data, editing it, resolving the conflict in it, while the real research sits under `.git/`. 002's repair (section 3.1 item 4: rename the real directory aside, relink) is right for the idle states and wrong in the middle of a merge or cherry-pick: relinking while `MERGE_HEAD` or `CHERRY_PICK_HEAD` exists would move the conflict's files away from the index that is tracking them. The replacement is a rule, not code: `instantiate` refuses, naming the operation, while any of `MERGE_HEAD`, `CHERRY_PICK_HEAD`, `REBASE_HEAD`, `rebase-merge/` exists, and a unit test for each. Unprobed beyond the above; impression for the exact refusal conditions.

This weakens C further without being a loss: tools at `mock/research` read the wrong content during any trip to an old commit, as they do now. In nested placement (2.1, 2.2) the same trips leave the data untouched at a different path and the tools that read the data path read the data.

### 2.4 The snapshot recipe has a cost nobody measured, and it is in both files

002 section 3.2 item 7 and my 001 section 3 item 6 give the same recipe: `GIT_INDEX_FILE=<tmp> git add -A && git write-tree && git commit-tree`, and 002 item 8 takes it "on every engine invocation that looks at the data". A fresh temporary index (my recipe adds `read-tree HEAD` first) carries no stat information, so `add -A` rehashes every file. q3 (`q3_output.txt`): 3000 files of 20 KB, fresh temporary index 0.211 s, a copy of the worktree's own index 0.018 s, the two trees equal. About twelve times, one run, one machine, not a bench, and the shape is unambiguous: it grows with the data, and research directories get reference assets (`000_op.md:45`). Replacement: copy the data worktree's index into the temporary index file first, then `add -A`. The recipe's output does not change; only its cost does. This corrects both files.

### 2.5 Dismissal of the separate clone rests on a mischaracterisation

002 section 2.3 F calls the separate repository "a second remote per project". It does not need one: `git clone --single-branch -b mock/<ns> <same url>` has the same remote. My reason for rejecting it (001 section 2 D: repo-level config, merge-driver definitions, `core.hooksPath`, `info/attributes` do not follow into a second repository) is the real one, and 002's p5 neither tests nor contradicts it. Keep F rejected, for my reason, not theirs.

### 2.6 One shared mutable directory across linked worktrees is a concurrency claim with no probe

002 section 3.1 item 3 makes one data worktree serve every linked worktree. Two agents in two linked worktrees edit the same files at once, and the landing gate (002 and the round at line 35: "`Mock-Ref` ... for each declared ref") reads a data state that includes the other's uncommitted edits. Whether that is the intent ("shared directory of a repository") or a defect is op's to say; nothing in 002 or its probes shows what happens to a landing gated against the other worktree's half-finished edit. Impression; owed probe: two linked worktrees, an edit in one, `data-refs verify` in the other.

### 2.7 Shared framing, not corroboration

Both files propose relocating the templates into the data tree and both propose the temporary-index snapshot. We are two readers of the same brief, so that agreement is not evidence (the standing rule on agreement between unratified artifacts). The template relocation rests on nobody having read the loaders: 002's probe 1 and my probe 4 are the same unrun question. I count it as an open fork for op, not a settled finding of the panel.

## 3. What stands after both files

Where the two proposals agree and I have measured it: one ref form (a branch), the data is a native worktree of that branch, `update`/`commit`/`push` are git commands run in it, merge drivers and LFS and signing and hooks come for free (002 p5, my p5, p6), the sparse machinery and the hand-built index, backup, pending, overwritten, stats, walk, lines, hook all go, and the recovery cache is a ref-held snapshot plus a one-day user directory taken before any engine step that writes (with 2.4's index copy). I hold that, and so does 002.

Where they differ, and what the evidence now says:

Placement. Three readings, none free of cost, ranked by measured loss:

- Reading N (nested, not hidden, same depth, disjoint path). Data physically in the tree, so relative paths in the data work (2.1). Immune to every verb run in the main worktree and refuses `worktree remove` and `clean -fd` (q4, my p2 control row). Costs: `?? <path>` in status, the `git add -A` gitlink hazard (a hook can refuse it), the path move to a path no history tracked (op's call; same depth), and one data branch per worktree if linked worktrees each need it (001 section 3 item 2), with the divergence cost 1.3 above. This is my build.
- Reading S (002's C, symlink). Immune to the main worktree's verbs and to `worktree remove` (nothing but a link is in the tree). Breaks every `..` path in the data (q2 a1, a2: 13 of 24 research manifests here). The link is destroyed by seven of eight old-commit verbs and the person works in a wrong directory until relinked, and may be mid-merge (2.3). One data worktree for everything, which is simpler. I would take S only for directories whose files never refer out of themselves, and I have not found one in this repository.
- Reading A (the shipped shape, corrected). Same path, hidden, sparse. Loses plain `checkout OLD` (002 p2, my p2), `worktree remove` (002 p6), `clean -fdX` (002 p6), and hides behind a hook the person can displace (002 1.3). Hold nothing for it.

What would distinguish N from S: a count of research files that contain a path back into the main tree. This repository gives 13 of 24 manifests. If op's consumers' research holds no such paths, S is viable and is simpler (one data worktree, no per-worktree branch). If op's own repository does (it does), N.

Linked worktrees under N, two sub-readings: N1 each linked worktree has its own nested data worktree on its own local branch (my 001 item 2), N2 only the primary has one and linked worktrees read it through a link. N2 reintroduces S's `..` break in linked worktrees, and worse, silently, since a physical `..` from the primary's data lands in the primary's tree, which may be a different branch of the sources: a build that reads the wrong crate and succeeds. I would not build N2. N1's cost is the branch count, which git carries. Probe owed: two linked worktrees each with a nested data worktree on its own tracking branch (the probe I listed as 9 in my file), and q4's remove-refusal repeated per worktree.

## 4. Probes that would settle what neither file did

1. Relative paths in data: count, across every repository that holds a research directory, files whose content names a path outside the data directory. Decides N against S (3 above).
2. Two linked worktrees, one nested unhidden data worktree each, per-worktree tracking branches: edit, push from one, pull in the other; and `worktree remove` on each. Decides N1.
3. The merge-in-progress rule (2.3): `instantiate` relink during `MERGE_HEAD`, `CHERRY_PICK_HEAD`, rebase, under S.
4. The two-worktree landing gate against a shared data worktree (2.6), under S.
5. A bench for the snapshot, in the bench harness, with arms: fresh index, copied index, `git stash create -u`-style, and the engine's own `backup.rs` path, at 100, 3000 and 30000 files. q3 is a spike with one competitor; it prices nothing.
6. Reftable (my 001 probe 1, 002 probe 7): the same question and still unrun.

## 5. Judgment on 002 as a document

It is the better of the two on placement and on the framing of P against Q (002 section 2.1), and its probes p1 to p6 are honest spikes that show what they claim. It fails where it recommends the symlink on one file's worth of evidence while listing the decisive probe as owed, and it fails the standing test gate by not running the suite. The panel should not take its section 3.1 item 3 as built. Everything else of its proposal I would take.
