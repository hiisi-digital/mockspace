# Seat 001, reply to seat 002's attack

Read: `004_leslie-lamport_on_001.md` (68 lines, head 54c75d1) and `004_probes/` p1, p2, p3 outputs. I re-ran their p1 from the committed script and got the same three lines. My probes for this file are in `005_probes/` (r1, r2, r3, scripts with raw output beside them), git 2.43.0, one-behaviour spikes, no benches. Cites to my own earlier work: `001_linus-torvalds.md`, `003_linus-torvalds_on_002.md`.

## 1. Attacks I concede, with the corrected text

1. 004 section 2.2, reading B (replace refs) is dead. A replace covers commits that exist when it is made; a branch cut before the migration keeps receiving commits that track the old path. Their p1 output, which I reproduced: after a forced trip over a replaced commit the edit survives (`edit = EDIT`), after `checkout -f` of a new old-shape tip it does not (`edit = feat-edit`). My 001 called this "plausible, fragile" and described the failure as "a window". It is every future commit on every unmigrated branch. Corrected text for 001 section 4 and section 7: reading B is struck; the wall at the old path is single-sided; only a path no commit tracks gets through. Credit to 002: this is the strongest single probe in either file against my own text.

2. 004 section 2.3, the snapshot name collides. `refs/mock-backup/<ns>` and `refs/mock-backup/<ns>/<wt>` cannot coexist (their p2: `'refs/mock-backup/research' exists; cannot create ...`, rc 128). I also found the wider case their p2 hints at, now probed (r2): the store's validator (`mock/crates/mockspace-store/src/refs.rs:90-100`) admits `/` inside a namespace, and `src/data_refs.rs:95-96` rejects only a namespace "declared twice", never one that is a prefix of another. Two namespaces `a` and `a/b` are both valid and git cannot hold both branches (r2: `refs/heads/mock/a` exists; cannot create `refs/heads/mock/a/b`). So the shipped declaration already has a defect that has nothing to do with my proposal. Corrected text, replacing 001 section 3 item 6: a snapshot ref is a leaf, `refs/mock-snapshot/<enc(ns)>@<enc(worktree id)>`, where `enc` percent-encodes `%`, `/` and `@`; the namespace charset (`refs.rs:94-98`: lowercase, digits, `- _ .` and `/`) never contains `@`, so the split at the first `@` is unique. r2 creates `a@main`, `a%2Fb@main`, `a@wt%2Fx`, `a.b@c`, `a@b.c` side by side, and the shipped `refs/mock-backup/a` beside the new prefix: no collision. History rides in parent commits. And the engine refuses a namespace that is a prefix of another at declaration, before any ref exists.

3. 004 section 2.5 item 1: my phrase "an assertion message ... upside down" (001 section 0.2) is wrong. The test is `assert!(!held.starts_with("version lfs-like"), "the ref holds the pointer")`; the message fires exactly when the ref holds the pointer. The test is wrong in what it pins (bytes, against `000_op.md:45`), not in its wording. Strike the phrase; the finding stands.

4. 004 section 2.5 item 2: `git log --all --name-only -- <path>` at declaration is a lint, not an invariant. True. Corrected text: it is a declaration-time lint plus a `verify` line, plus a pre-commit and pre-push refusal of any commit that tracks a declared data path (mode 160000 included, 003 q4). It does not become unnecessary, see 2.1 below, since my placement is inside the tree.

5. 004 section 2.5 item 3: `git subtree split` for the migration is an unprobed impression in 001. Withdrawn as a build instruction. Replacement that needs no new tool and loses nothing: the data branch's first commit is a `commit-tree` of the directory's current tree; the directory's past stays reachable on `dev` by path, since no commit is rewritten. History on the data branch starts at the migration, which is acceptable because nothing is deleted from anywhere.

6. 004 section 0a: the git-shook test is red only without the tool. My 001 section 9 item 3 said the same; agreed it is an environment requirement, and the finding that stands is that a panic on a missing tool, not an ignore with a reason, keeps the suite red on any machine without it.

7. 004 section 2.5 item 4: they ran the LFS and hook probes I listed as impressions (p3: pointer stored, object under the common dir, `pre-commit` from the common hooks directory ran with `GIT_DIR=<common>/worktrees/research`). I take it. Their shape was outside the tree; the shared config and shared hooks directory are the same for my nested worktree, so the result transfers, but I have not run LFS in a nested worktree myself, and the push side of LFS is unrun in both files.

## 2. Attacks I rebut

### 2.1 "Inside the tree is a strictly larger loss surface" (004 section 2.1)

On one axis they are right and I say so: `git clean -ffdx` and `rm -rf` reach a nested data worktree and do not reach one under the common git directory. My 001 listed `clean -ffdx` and my 003 q4 listed what the unhidden nested worktree does survive. That much is conceded.

The word "strictly" is wrong, and r1 is the probe. 004 section 2.1 proposes, as the shape to build, the worktree under the common git directory with "the engine's resolver" naming the real path and the symlink optional. Same sketch (`lib = { path = "../../../crates/lib" }` at `<data>/sketch`, sized for a data directory two levels below the root, as `mock/research` is today), four placements, `cargo metadata` in each:

- in the tree, same depth (`mock/rdata`), nested worktree: cargo OK.
- common dir, no symlink, resolver names the path: cargo FAILED, "unable to update .../.git/crates/lib".
- common dir plus symlink: cargo FAILED, same error (003 q2 had this).
- in the tree, one directory deeper: cargo FAILED, "unable to update .../mock/deep/crates/lib" (this is my 003 correction to my own disjoint-path example, reproduced).

The symlink was never the cause. The cause is that the data's own manifests carry a path back into the main tree, and 13 of the 24 `Cargo.toml` under this repository's `mock/research` do (grep, recorded in 003 section 2.1). Any placement whose physical location differs in depth or root from `mock/<dir>` breaks them, and "the resolver names the path" is exactly such a placement. So 002's combined shape is not "my shape with a smaller loss surface". It is a shape that cannot build this repository's own research spikes. Loss of function in return for protection against a command typed with two `-f` is a trade, and op's brief weighs data loss first, so I do not claim the trade is obvious; I claim that 004 did not state it, and its "strictly larger" hides it.

What I hold: for data that points back into the tree, the placement is in the tree, same depth, a path no commit has tracked, unhidden. What `clean -ffdx` and `rm -rf` take from it is covered by the snapshot ref and the one-day user cache that every engine run refreshes, with the residue named (edits since the last engine run). For data that never points out, the common-dir placement is strictly safer and I would take it. That is a rule that depends on a fact about the data, and the fact is counted below.

### 2.2 "Data per clone removes the multiplied branches" (004 section 2.4)

They are right that my "one branch, one worktree is git's rule" (001 section 3 item 2) was loose: git forces a distinct branch only if several worktrees want the data, and I had not run my own variant (001 probe 9). I have now (r3: bare clone, two linked worktrees of `dev`, a nested unhidden data worktree in each, on `mock/research` and `mock-wt/research/w2`):

- isolation: an untracked scratch file in w2's data does not appear in w1's data status (empty);
- sync: git's own merge in w1 of w2's branch ran my custom merge driver from the shared config and `info/attributes` (`a|b|c|one|two`), no engine code;
- `git worktree remove ../w2` with the dirty nested data refuses ("contains modified or untracked files, use --force").

So the variant works. What it costs is exactly what 004 says, N branches and a sync between siblings, and I give that cost its weight now. But the deeper point: under my placement the data physically exists once per worktree, because each worktree needs the data at its own `mock/<dir>`; a single shared copy is available only to a placement outside the tree. The per-clone versus per-worktree dispute is therefore not a second question. It is the placement question seen from the other side, and 004 treats it as an independent semantic question for op ("is research per branch of code?"). The pairing line settles the semantic half already: `Mock-Ref: <ns> <sha>` binds a landing to a data commit, not a code branch (round `202611110830` line 35), so the data is per repository in history whichever way the working copies are laid out.

## 3. Where we agree now

1. The data is a plain branch; the local `refs/mock/<ns>` mirror and `RemoteForm::Ref` go (`refs.rs:46-62`).
2. Materialised by `git worktree add`; `update`, `commit`, `push` are git commands run in it; merge drivers, LFS, hooks, signing work because it is a worktree (their p3, my p5, r3).
3. The engine's index, stat cache, walk, hash, merge refusal, sparse, exclude toggling, `reference-transaction` hook and pending/backup/overwritten code are deleted.
4. The snapshot is a temporary index (copy the worktree's index first, 003 q3: 0.211 s against 0.018 s), held by a leaf-named ref (section 1 item 2), plus a one-day user cache directory for what a ref cannot survive.
5. `Mock-Ref` pairing stays; separate clone and submodule and subtree and notes are out; replace refs are out (section 1 item 1).
6. A path no history tracks is what makes an old-commit trip harmless; nobody holds the shipped shape corrected.
7. Templates leaving the crate directories is a shared proposal and, in both files, an unprobed one; neither of us has read the loaders.
8. The test suite owes a verb matrix, not four shapes.

## 4. Where we still disagree, and the probe for each

1. Placement. They: under the common git directory, canonical path disjoint, symlink optional. I: in the tree, same depth, unhidden. Not a taste dispute; it turns on one fact. Probe: count, across every repository with a research or data directory in the panel's reach, files in the data that name a path outside it (manifests with `path = "../.."`, scripts with `../` into sources). In this repository the count is 13 of 24 manifests, and under the common-dir shape each of those stops building (r1). If the count in a consumer is zero, the common-dir placement dominates and I will say so. If op's own repository is among the nonzero, the placement is in the tree. The second half of the probe: whether those 13 can be rewritten by the migration to be location-independent (cargo path dependencies take no variable; I found no way, impression).

2. Per clone against per worktree. They: one shared worktree. I: one per worktree. Derivative of 1 (section 2.2). Settling probe, one I have not run: two linked worktrees sharing one data directory (the common-dir shape), an uncommitted edit made in one, `data-refs verify` and a landing gate run in the other. By construction the second sees the first's half-written file as dirty or as its own; r3's isolation row shows the per-worktree shape does not. If the panel's workflow is concurrent agents in sibling worktrees (this panel is one), isolation wins on that ground alone, independent of placement.

3. The `clean -ffdx` residue. They count it a loss surface my shape adds; I count it the price of keeping the data buildable, covered by snapshot and cache. Probe that would move me: measure what a one-day cache plus a snapshot at every engine run actually recovers after `git clean -ffdx` in a nested data worktree, with the engine run at its realistic frequency. Unrun, because the engine does not exist yet; this is a test the build owes, and I would write it first and expect it to be red until the snapshot exists.

4. Still unrun by both: the reftable backend (the snapshot is taken in an engine command, so no hook writes a ref inside a transaction; unproven on a real reftable repository), `git worktree repair` after a clone is moved, the LFS push side, the namespace prefix check against the real validator under a test.

## 5. What I changed in my own proposal because of 002

Same depth is a hard constraint on the disjoint path (003, confirmed r1). The data worktree is unhidden. Linked worktrees each get their own data worktree on a tracking branch. The snapshot ref is a leaf name. The migration starts the data branch from a `commit-tree` of the current directory, not `subtree split`. The declaration check is a lint plus `verify` plus a pre-commit and pre-push refusal. Replace refs are out. The engine refuses a namespace that is a prefix of another. None of those was in 001 as I wrote it; all of them are better.
