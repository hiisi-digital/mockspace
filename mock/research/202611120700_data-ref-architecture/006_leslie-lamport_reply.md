# Seat 002 replies to seat 001's attack (003)

Read: `003_linus-torvalds_on_002.md` and `003_probes/` q1 to q4 outputs (origin head 54c75d1). I re-ran none of 001's scripts. Where I rely on a 001 output I say so. My probes for this reply are in `006_probes/`, git 2.43.0, cargo from `/home/user/target` toolchain, one cargo process at a time, nothing killed. Each is a spike of one behaviour and prices nothing.

## 1. The main attack: `..` through the symlink. Conceded, with one narrowing

Counts verified in my own worktree: `find mock/research -name Cargo.toml` gives 24, and 13 of them carry `../../..`; the dependency is `path = "../../../../bench-harness"` in 11 of those (for example `mock/research/202608151554_probes/mode-is-a-string/Cargo.toml:8`), with `bench-core` and `bench-macro` at five and four `..`. 001 is right on the number.

Probe `006_probes/p1_output.txt`, my own build of the same shape (a research crate at `<data>/sketch` with `lib = { path = "../../../crates/lib" }`, data under `.git/mock-data`, linked in at `mock/research`), four invocations and a control:

- A, `cd mock/research/sketch && cargo metadata`: FAILED ("failed to load manifest for dependency `lib`").
- B, from the root, `cargo metadata --manifest-path mock/research/sketch/Cargo.toml`: OK.
- C, shell `cat ../../../crates/lib/Cargo.toml` inside the link: FAILED.
- D, `cd` into the link, then `--manifest-path "$PWD/Cargo.toml"`: OK.
- E, control, a real directory: OK.

So 001's q2 reproduces under a different build of the fixture, and it also narrows: the break is every invocation whose process cwd is inside the link (A, C), because the kernel's cwd is physical, and not an invocation by a path that is normalised lexically (B, D). The research probes here are standalone crates, each with its own `Cargo.lock` and `output.txt` (`mock/research/202608151554_probes/mode-is-a-string/`), and the shape of their manifests (no `[workspace]`, a relative path out) says they are run by `cd`. That last step is an impression; I did not find the runner. On it, case A is the usual case, so the narrowing does not rescue the symlink for research. I withdraw `002` section 3.1 item 3 (the link in every worktree) and section 3.1 item 2's placement for any directory whose files name a path outside themselves. In its place, the correct text of that item is:

"The data directory is a real directory physically inside each work tree at the depth of the path it replaces, on a path no commit of any ref has tracked. It is a nested worktree of the data branch."

I also concede the fault that goes with it: `002` section 4 listed directory-symlink behaviour under cargo as an owed probe and section 3 proposed building on the symlink before it was run. A proposal that depends on a probe is not made before the probe.

One thing I keep, since it is a measured fact and not a position: a symlink fails loudly (A: cargo errors, C: "No such file"), where 001's own N2 variant (primary has the data, linked worktrees read it through a link) can build the wrong crate silently, as 001 section 3 says of N2. I agree with 001 that N2 is not to be built; the loud failure is still the reason the symlink is a refusal and not a hazard.

## 2. The link is destroyed in seven of eight verbs, and mid-merge it is the wrong directory. Conceded

001's q2 b1 to b8 extends my six ways to eight and the data edit survives all eight (001 prints `data edit survives=1` for each); I take that as confirming my `p4` over a wider matrix. It also shows the link gone in seven (`link still a link=NO`), and in `merge` (b3) and `cherry-pick` (b8) the replacement directory holds a modify/delete conflict in progress (b8: "Version fb5cc34 (old branch edit) of mock/research/NOTES.md left in tree"). My `002` 3.1 item 4 (rename the real directory aside, then relink) is wrong at that moment: it would move conflict files away from the index that tracks them. I take these outputs from 001, not from a run of mine. Corrected text, for any placement that has a link:

"`instantiate` refuses, naming the operation, while `MERGE_HEAD`, `CHERRY_PICK_HEAD`, `REBASE_HEAD` or `rebase-merge/` exists in the git directory of the worktree, and has one test for each."

Under the placement I now accept (section 1), there is no link and the rule is not needed.

## 3. The snapshot rehashes every file. Conceded, and checked

001's q3: fresh temporary index 0.211 s against a copy of the worktree's own index 0.018 s, 3000 files of 20 KB. `006_probes/p2_output.txt` reproduces it with a script of mine: 209, 193, 207 ms against 17, 14, 12 ms, trees equal. Both are spikes with two arms; the shape is stable and the figure is not a price (001's own point, section 4 item 5: the harness has not run). My `002` 3.2 item 7 recipe starts the temporary index empty and should read:

"Start the temporary index as a copy of the data worktree's own index, then `git add -A`, `write-tree`, `commit-tree`."

I checked the one trap in that: git's racy-clean case. A copied index could give a stale snapshot for a file edited at the same size within the same second as the index write. `p2_output.txt`, last line: the snapshot carried the new bytes (`BBBB`). That is one case; I claim nothing about the others. The reason it holds is git's own smudge of racy entries when the index is written, which a `cp` carries unchanged (impression about the mechanism, the result is the probe).

## 4. The remaining attacks

1. 001 2.2 and q1: a hidden nested worktree is deleted by `git worktree remove` of its parent, unhidden is refused (q4). I did not test that shape for the hidden case in `002`; my `002` p1 had the nested worktree hidden and I did not run `worktree remove` on it. 001's measurement stands, and it is also what `002` p6 found for the engine's own shape. The unhidden nested placement is the better protected of the in-tree placements.
2. 001 2.2 residue: an unhidden nested data worktree lets `git add -A` record a 160000 entry on a code branch, and 001 proposes a pre-commit hook to refuse it. A hook is best-effort in the sense of my `002` 1.3. I probed what the committed gitlink costs when the hook is absent: `006_probes/p3_output.txt`, the gitlink committed to a branch `polluted`, then `checkout -f` to and from it, `reset --hard`, `switch --detach`, `clean -fd`: the uncommitted edit survives all six, the worktree record is kept. So a failed hook costs history pollution, not data. That closes the question I wanted to raise against 001, and I withdraw it as a data-loss claim. It stays a tidiness cost.
3. 001 2.5: I wrote that a second repository "costs a second remote per project". That is wrong: `git clone --single-branch -b mock/<ns> <same url>` uses the same remote. I keep F rejected, but on 001's reason (repo-level merge-driver, filter and hooks configuration does not follow into a second repository, which 001's p5 shows does follow into a worktree), not on mine.
4. 001 2.6: one shared data worktree across linked worktrees is a concurrency claim with no probe, and the landing gate would read another worktree's half-finished edit. True; I made the claim from `p5` which tested a read of the same bytes, not two writers and a gate. Under the placement I now accept it is moot (each worktree has its own), and it stays an owed probe only if op asks for a shared directory.
5. 001 2.7: agreement between our two files is not corroboration, and the template relocation rests on nobody having read the loaders. Agreed on both. What is independent is the probes: 001's p2 control row and my `p4` measured the same thing separately and agree, 001 q4 and my `p2` agree on the `clean -ffdx` case. The proposals agree because we read the same brief.
6. 001 section 5: I did not run the whole suite. Correct, and the gate was therefore only partly discharged, as I said at the top of `002`. 001's whole-suite number (five red) is the better figure.

## 5. Where 001 and I now agree

- One local branch form for data refs, `refs/heads/mock/<ns>`; the `refs/mock/<ns>` local mirror and `RemoteForm::Ref` go (`mock/crates/mockspace-store/src/refs.rs:46-62`).
- The data is a native worktree of that branch; commit, update, push are git commands run in it; merge drivers, LFS, signing and hooks are git's own (`002` p5, `004` p3, 001 p5 and p6).
- Placement: physically inside the work tree, same depth as the path replaced, on a path no history tracked, and not excluded. This is 001's reading N; I now hold it for directories that name paths outside themselves.
- Reading B (replace refs) is struck: `004_probes/p1_output.txt`, a new old-shape commit on an unmigrated branch collides again. 001 did not contest this in 003.
- The recovery cache: a ref-held snapshot from a copied index, plus a one-day user cache directory for what a ref cannot survive. Leaf ref names that cannot collide with `refs/mock-backup/<ns>` (`004` p2).
- The separate clone is rejected, for the configuration reason.
- The template relocation is an open fork for op, not a panel finding.

## 6. Where we still disagree, and what settles it

1. The residual for the double-force. Under nested placement `git clean -ffdx` and `worktree remove --force` run in the main tree delete the data worktree's uncommitted edits (001 p6, q1; my `002` p2). 001 calls that git's own list for any repository. I hold that it is a larger loss surface than the out-of-tree placement had for the same data, bought at the price of the `..` paths, and that the panel should say it is a trade and not call it equal. Not a probe question: a double `-f` is the person asking. What I would add is a mitigation measured rather than argued: the snapshot taken at every engine command (cheap with the index copy, section 3) bounds what a double-force can destroy to the work since the last engine command. Whether that bound is acceptable under op's "impossible to lose" is op's call.
2. Per-worktree data branches against one shared copy. Under physical nesting each linked worktree needs its own nested data worktree, so N1 (001 section 3 item 2, a local branch per worktree, `mock-wt/<ns>/<name>`) is forced, with the divergence cost 001 itself named in 003 1.3. I hold that sharing was what "shared directory of a repository" meant; 001 holds that the `..` break outweighs it. What settles it: op's meaning of "shared" (a question for op), and the unrun probe both files list: two linked worktrees, each with a nested data worktree on its own tracking branch, edit in one, push, pull in the other, and `worktree remove` on each. I have not run it and neither has 001.
3. The path move. Both of us now require the data to move to a new name at the same depth. 001 says op names it. I add that the `no-legacy-shims` rule forbids an alias at the old name, so the old `mock/research` name vanishes for every reader. The census of readers of `mock/research`, `research/`, `*.md.tmpl` and `design_rounds` (001 section 3 item 9 lists files) has not been done by either of us. It settles how big the move is, and it is the same census that settles the template relocation.
4. The research dependency paths. 001 treats the 13 manifests as a fixed fact about the data; I note they are a property of how those probes were authored. If research probes were authored with a path from their own directory to a workspace root that they find by a variable, no placement would break them. That is a change to the data, not to the engine, and it is not mine to propose; I record it as the one way the symlink could become viable again and mark it as nothing more.
5. Reftable. Neither of us has git 2.47; both list the probe. Under nested worktrees with the snapshot taken by an engine command and no hook, the lock question of `202611110830:79` does not arise, which is a reading and not a measurement.

## 7. Judgment on 003

Its main attack is right and its q2 is a better probe than my `p4`: it put a relative path into the data, which is what the repository's data does, and I did not. Its snapshot cost and `worktree remove` findings are right. Its dismissal of the symlink as refuted for research is correct on the repository's evidence and is, on invocation by cwd, reproduced by my own `p1`. Its claim that the symlink "breaks every `..` path" is broader than the measurement: lexical invocation by `--manifest-path` works, which I report so that a later reader does not treat B and D as impossible when a tool uses them.
