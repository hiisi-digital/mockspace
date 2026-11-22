# Seat 002 on op's second answers: the two-way shadow tree, the purge epoch, the state directory

Read: the end of `000_op.md` ("Answers to the revised consolidation", op's words) and `014_consolidation.md` (glossary, 2.2, 2.3, 2.6; the rest not audited here). My probes are in `016_probes/` (p1 to p4, scripts with raw output). Every probe makes its own scratch directory, refuses to run outside it and removes it on exit; the git operations ran under `timeout`; cargo (p3 only) ran offline with its target directory inside that scratch. Git 2.43.0 on Linux, `file://` remotes. Each probe is a spike that prices nothing. Nothing was killed.

Gates. Canon gate: no `canon_paths`; op's answers govern, and each owed item below is stated from them. I am one reader and a second is owed. Test gate: the whole suite was never run by me, as before.

Settled by op and taken as given: the instance root is `.mock` (`git~1` dropped), git-shook is auto-installed, a purge goes through a purge epoch, there is one shadow tree under a name that is not `@` and not any name git reads, and the shadow tree is two-way.

## 1. The two-way shadow tree

### 1.1 What it is, in the model

The simplest form that satisfies op's words is to make the shadow tree a mock tree of its own and not a directory `@` inside one. Call its kind `shadow`: a mock tree whose root corresponds to the root of the git tree. Then the correspondence between a shadow path and a git-tree path is the identity on the directory part, `shadow://<name>/crates/foo/README.md.tmpl` pairs with the git-tree directory `/crates/foo`, and no reserved inner directory name is needed at all. That removes the question of which name git reads: the only name left to choose is the namespace, an ordinary ref component and an ordinary directory under `.mock`. Names to avoid for it, because git gives them a meaning as a revision or a placeholder: `@` (probed in `013_probes/p4`: `git log @` is "both revision and filename"), `HEAD`, `FETCH_HEAD`, `ORIG_HEAD`, `MERGE_HEAD`, `CHERRY_PICK_HEAD`, and `-` (the previous branch). Only `@` was probed; the others are an impression from git's documented revision syntax. The name is op's.

One shadow tree per git tree. Every file in it is a source of the engine: a template, or its kin. Physical consequence, already noted in 014's open item: relative paths inside a shadow file to git-tree neighbours do not resolve physically from `.mock/<name>/...`, because the depth differs; the generator resolves them in the git-tree space, and a tool that reads a shadow file by its physical path does not.

### 1.2 The states, per generated output

Fix one output path q of the git tree. Write:

- T, the source blob or tuple of blobs at the shadow tree's tip that generate q (the template);
- Gen, a pure function from T and a context C (the git tree's other files and the manifests) to bytes; it is required to be deterministic;
- B, the baseline: the last generated result the engine wrote for q. It is stored in the shadow tree itself, in a generation record the engine owns (one file per output), so that it travels with the template. The alternative, recomputing B from the template version a record names, is cheaper in space and fails when the generator changes; storing the bytes is the choice that survives a generator upgrade. This is taste with that stated reason;
- O, the tracked output at HEAD of the git tree;
- E, the upstream edit, the diff from B to O. It is empty iff O equals B.

State of q: clean (O equals B and Gen(T, C) equals B); stale (O equals B, Gen differs); edited (O differs from B).

### 1.3 The transitions

`regenerate` is the only verb that writes O, B or T, and it acts per output:

1. clean: nothing.
2. stale (no edit): O and B are set to Gen(T, C).
3. edited: attempt `carry back`.
   - Carry back computes the template after the edit as the three-way merge of ours = T, base = B, theirs = O, by git's own `merge-file`: the diff from B to O is applied to the template, aligned by lines.
   - It computes what regeneration would give with the edit kept, O' = the three-way merge of ours = Gen(T, C), base = B, theirs = O.
   - It accepts iff both merges are clean and Gen(T', C) equals O'. The round trip check is what makes the carry-back exact and not merely plausible.
   - On accept: T := T', B := Gen(T', C), O := Gen(T', C). The edit is now in the template and the output equals the baseline again.
   - On any other result the output is in a conflict state and nothing for q is written.

`accept an upstream edit into the template` is the accept branch of carry back. `conflict` is the other branch, and it persists as a visible state: `status` names q, the hunk and the class below, and `regenerate` for every other output proceeds.

A mode matters. Between a pull request landing and a maintainer's next regeneration, the output must carry the edit and survive other regenerations. That is not a separate state: an edited output that carry back has not yet been asked to absorb is protected by the same rule that protects a conflict, namely that O is never overwritten while it differs from B. Where the template changed in a way that does not touch the edited lines, the regeneration of O' preserves the edit and moves the rest, and that result is what is written when carry back is declined (`--no-carry`) or still pending.

### 1.4 Invariants, and how each is held

- N1, no edit is lost. O is written by regenerate in two cases only: it equals B (there is no edit to lose), or carry back accepted (the edit is in T' and Gen(T', C) reproduces it byte for byte). Otherwise O is untouched. So an edit is always in O, or in the template with a reproduction check, and is never dropped silently. Holds by the rule; exercised by `016_probes/p1_output.txt`.
- N2, regeneration is idempotent. After an accept, O equals B equals Gen(T', C) and T is unchanged, which is the clean state, so a second run writes nothing. In the preserving case the second run's three-way merge with B := the new generation and O := the result returns O: `p1_output.txt`, "regenerate a second time ... R2 == R: yes". In a conflict state repeated runs write nothing and report the same hunks.
- N3, an edit survives every regeneration until it is carried back. Follows from N1: until carry back accepts, O keeps the edit, either by the preserving merge or by being untouched.
- N4, the edit is never presented as a change to a thing that is computed. A hunk that touches a computed line is a conflict, not an edit to the template that would then be regenerated away. That is the point of the round trip check.

### 1.5 The round trip, probed

`016_probes/p1_output.txt`, a deterministic generator (substitution of two placeholders and a loop line), the template, a baseline, and edits to the output:

Automatic (carried into the template, Gen of the new template equals the edited output): a literal line edited; a literal line edited plus a line appended; a two-line literal paragraph deleted; an insertion directly after a literal line and two lines above a computed one; text appended after the last line; two independent literal edits made in different places (as from two pull requests merged by git first).

Not automatic (a conflicting hunk): an edit on a line the template computes (placeholder lines, the title); an edit inside a loop-expanded region; a line added at the end of a loop-expanded region; and, the boundary worth stating, an edit on the line directly after a computed line. Touching hunks conflict even though the edited line is itself literal.

Regeneration with an edit present: a literal edit plus a version bump, merged by `merge-file` with the new generation as ours and the baseline as base, keeps both (the edit and the new version appear in the result), and a second regeneration returns the same bytes. A literal edit overlapping the template's own change to the same line conflicts.

Git's own merge of the output between branches (`016_probes/p4_output.txt`, no driver): two branches that both regenerate the same bump merge with no conflict, a pull request's literal edit and a regeneration merge with both present, and a pull request's edit to a computed line against a regeneration of that line conflicts, with the pull request's text still in the working file. Those are the two "trivial conflicts" op named: the first never arises, because identical changes merge, so regeneration produces no merge conflict when both sides regenerated the same thing.

### 1.6 Exactly when it cannot be automatic

From the probe and the construction:

1. The edit touches or is adjacent to a line the template computes: a placeholder expansion, a loop or conditional expansion, an include. The right destination of such an edit is often not the template but the source of the computed value (a version field, a manifest); the engine says which source the line is computed from when it knows, and otherwise only names the line.
2. The edit and a change to the template overlap the same lines.
3. The merge is clean but the round trip fails (Gen of the carried template differs from the edited output). This catches template constructs that are not line aligned.
4. There is no baseline, or the generator is not deterministic, so B cannot be trusted. A migration seeds B := O only where O equals Gen(T, C); otherwise the output is a conflict until a person decides.
5. The output is not line text (binary, or reflowed or minified text), where a line merge is meaningless.
6. The output is assembled from several sources and the edit falls in a region owned by more than one: the line alignment gives one candidate source and the round trip may not confirm it.

### 1.7 A defect in the shipped merge driver, which this design must replace

`src/bootstrap/driver_script.rs` implements the `mockspace-generated` merge driver. Its header says it "keeps ours" for a generated file when conditions hold, and a comment at `:116` says "a conflicting hunk keeps ours, to be settled from the merged manifests afterwards"; it records that it did so in a `mockspace-kept-ours` file (`:43`). Read as written (I did not run the driver; it is generated by the engine and needs its hook environment), a pull request's edit to a generated document that conflicts with a regenerated hunk is not kept by that merge. The edit is dropped in the working file, with a marker file saying a regeneration is owed. That is the exact edit op wants carried back. `p4` (c) shows what git alone does instead: it leaves conflict markers and the pull request's text visible. Under the two-way design the driver's rule for a conflicting hunk becomes: keep the conflict (or keep theirs' side written out as the edited O), never ours alone. That is a change to a shipped mechanism and so a design change, not an addition.

## 2. The purge epoch

### 2.1 The problem

Under the repair rule of `014` (2.6, layer 2) a remote ref that is absent or a proper ancestor of the clone's mirror ref is repaired by pushing the mirror ref. A purge publishes a tip that is not a descendant of the old tips, so a stale clone either finds a remote it believes incomparable, or finds the ref deleted and re-publishes its old history, including the secret. `014` says the purge is undesigned. This is the design.

### 2.2 State

- epoch(t) is an integer stored in the tree of a published tip, in a file `.epoch`; absent means 0. It is part of the tip's content, so it travels with the tip and cannot disagree with it.
- The extended order on published tips: a is at least b iff epoch(a) > epoch(b), or the epochs are equal and b is an ancestor of a or equal to it. Within one epoch this is ancestry as before.
- A clone's witness n_known is the largest epoch it has any evidence of: the epoch of its mirror ref, and the epoch named by the landing lines of the git tree it has fetched (a landing's `Mock-Ref` line carries the epoch of the tip it names). The git tree is a second, independent carrier of the epoch, fetched by clones through ordinary code fetches.
- A purge is a verb that publishes a new root: a commit with no parents, the secret absent from its tree, `.epoch` = epoch(r)+1, pushed with a lease on r.

### 2.3 The rules

Hook rule (layer 1): a push of a tip l to `refs/mock/<ns>` over r is allowed iff l is at least r in the extended order. This allows a purge and refuses every same-epoch non-fast-forward, a delete, and a push carrying a lower epoch. `016_probes/p2_output.txt`: the epoch-1 new root over an epoch-0 tip is allowed; a same-epoch rewind is refused.

Repair rule (layer 2), restated with the extended order. Let r be the remote tip, s the mirror ref, n = max(epoch(s), n_known):

- r absent: push s only if epoch(s) is at least n; otherwise refuse and adopt the purge on next contact.
- epoch(r) > epoch(s): adopt. Mirror := r; carry local-only commits forward (cherry-pick the commits reachable from the working ref and not from the old s onto r; one that conflicts stays in a local ref and is never pushed); delete every local ref and expire every cache entry whose tip has a lower epoch; collect.
- r at least s: nothing (or mirror := r).
- s above r in the order: push s with a lease on r. (This is also how a purged remote is restored after a stale clone pushes an old tip: the clone holding the higher epoch repairs it.)
- otherwise (incomparable within one epoch): nothing; `update` merges.

### 2.4 The probe

`016_probes/p2_output.txt`, real git objects, five clones: A purges; B is stale with a local unpushed commit; C stands for a plain clone that deletes the remote ref by mirror push; D is stale but has fetched the code tree (witness epoch 1); E is stale and has seen no witness.

- B runs repair: it adopts (no push), the remote stays at the purged tip, the secret blob is absent from B after collection, and B's local commit is carried forward (`carried: 1 of 1`, its note is in the working ref).
- After the remote ref is deleted: D refuses to republish ("s is epoch 0 < known epoch 1"); E, which has no witness, pushes its epoch-0 tip, so the secret is back on the remote ref; A then repairs by the extended order and pushes the epoch-1 tip over it; and A repairing first from a deleted ref restores the epoch-1 tip.
- Idempotence: further repairs by A, B and D leave the remote unchanged.

### 2.5 The proof that repair never undoes a purge

Let the published history be a sequence of tips with non-decreasing epochs, ordered by the extended order.

1. Engine pushes are at least the remote. The hook rule refuses any push l unless l is at least r; repair pushes only when r is absent and epoch(s) is at least n, or when s is above r (a lease on r). So an engine action cannot move the remote down the order, except by creating after a deletion.
2. A creation after a deletion cannot republish below the clone's witness. By the rule, a clone with n_known = n creates only a tip of epoch at least n. Any clone that has seen the purge, by either carrier, holds n at least equal to the purge epoch, so it never creates a lower-epoch tip. Any clone that has not seen the purge is the exception below.
3. A purge is observed on first contact. Every engine command that talks to the remote runs the repair rule first; if the remote's epoch exceeds the clone's, the clone adopts and scrubs and does not push.
4. Therefore, for every clone that has been in contact with the remote or the code tree since the purge, repair never undoes it. The one case not covered is a clone with no contact since the purge and no witness, which can create a lower-epoch tip when the ref is absent (`p2` E). Two facts bound it: such a clone already holds the purged content and no protocol could remove it from that clone, and any later clone that has the higher epoch overwrites its push by the extended order (`p2`, the repair after E). The secret is on the remote ref for as long as no clone with the higher epoch has touched it; and its objects are on the remote until that remote's gc regardless. So the honest statement is: a purge removes the content from every clone that makes contact after it, and from the remote ref after the next such contact. It does not and cannot retroactively remove it from a remote that kept the objects, which is the host's gc.

What a purge does to no-loss. Purge deliberately violates I1 for the purged objects, and only for them. Everything else that was reachable only through the old line (local commits, the working copy) is carried forward or kept in a local ref that is never pushed. The user cache and backup refs are scrubbed of the purged objects, since keeping them there defeats the purpose; that is the one place the recovery cache is not allowed to lean on itself, and op should know that a purge empties the cache of that history by design.

## 3. Could instances live in a platform state directory?

The question, from the formal model of `013` section 1, with `<state>` for `$XDG_STATE_HOME` or its platform equivalent and `.mock` kept as an ignored link in the git tree.

Probed (`016_probes/p3_output.txt`: the data worktree at `<state>/<key>/research`, `.mock` an excluded symlink to `<state>/<key>`):

- I3 (ref ruling) is unaffected: it is about refs, not paths.
- I1 (no loss) gets better in one respect and worse in another. Better: the data is out of reach of every command run in the git tree. With an uncommitted edit, `checkout -f`, `reset --hard`, `clean -fdx`, `clean -ffdx`, `stash -u` and `switch` all left it intact; `clean -fdx` and `-ffdx` removed the link and nothing else, and re-linking restored access. That removes the double-force clean and `rm -rf` of the clone from the loss paths of `009_probes/p1`. After `rm -rf` of the clone the data files stay in the state directory, edit included, though the committed history goes with the clone's object store. Worse: the state directory is a place nobody owns, and nothing removes it, so it needs an explicit `retire` and a sweep for orphans.
- I2 (git tooling unchanged) holds: the instance is still an ordinary worktree, and `git status` in the git tree shows nothing (`[]`).
- I6 (depth stability) breaks. A physical parent of the instance is then the state directory: `cd .mock/research/sketch` and `cargo metadata` breaks for a manifest with `../../../crates/lib`, and a `--manifest-path` invocation from the git-tree root resolves (`p3`, block b; the same result as `006_probes/p1`). So the state directory is viable only where no mock-tree file names a path outside its instance by a relative path. The existing research directory has 13 such manifests of 24 (`006` census). The depth rule that forced ρ to one component forces the instance to be physically inside the git tree for those files, so for the `data` namespaces that carry them, the answer is no. For namespaces that carry none, the answer is yes.
- I4 (disjointness) holds trivially: nothing under the state directory is a git-tree path, so no commit can track it. That is the property the refused name `git~1` was bought for, delivered without any name trick.
- Identity. Two clones of one repository need two instances; the key cannot be the repository's identity (the probe's second clone collided: "already exists", and one branch cannot be checked out twice) and cannot be the clone's path (a moved clone then has no instance). A key stored in the clone's own git directory, written at instantiate, survives a move; the instance's own `.git` file then needs `git worktree repair` after a move, which the probe shows works (`p3`, block d: after `mv` of the clone, repair restores the link, the edit is intact, the branch is right).
- Platform. `~/.local/state` is the XDG convention; macOS and Windows each have another place, and a symlink into it has the Windows caveats already noted. I did not test either.

So the answer: it can, for every mock tree whose files carry no relative paths outward, and it should not be the only placement, because the research directory in this repository is not such a tree. The design can offer both, chosen per namespace by a declared property ("this tree's files are position-independent"), which the engine can check at `instantiate` by scanning for relative paths that leave the tree. I would not make the choice implicit.

## 4. What stays open

1. The shadow tree's namespace name, and the reserved names of the generation record inside it (op's).
2. The round trip proof for multi-source outputs (item 1.6.6): not exercised.
3. A generator's determinism as an obligation: the engine can check Gen twice and refuse a non-deterministic one; not built or probed.
4. The merge driver replacement of 1.7, and whether keeping theirs or leaving markers is the default for a conflicting hunk. Op's "no trivial conflicts" is met by git's identical-change merge (p4 a); the non-trivial ones need a human.
5. The purge verb's authorisation (who may publish a higher epoch) and the case of a host that refuses a non-fast-forward at all, which would stop a purge, since the hook rule and the host's rule must agree. Unprobed against a real host.
6. Whether the witness in landing lines is available to a clone that never fetches the code tree (a pure data clone has no witness but the ref).
7. A real host, Windows, macOS: unchanged from `013`.
