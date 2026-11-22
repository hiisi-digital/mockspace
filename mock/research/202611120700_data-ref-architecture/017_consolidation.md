# Mock trees: final consolidation (seat 003, Fabian Giesen)

Status: this file is the canon candidate that the design rounds are cut from. It replaces `014_consolidation.md` and keeps its shape, with op's second answers folded in. It is unratified. Op's words in `000_op.md` govern, in four sections:
- "During the audit";
- "Answers to the consolidation";
- "The registry stores";
- "Answers to the revised consolidation".

Where a seat file and this one differ, both are cited so op can read the evidence directly.

## Gates and evidence

Canon gate. There is no `canon_paths`, so the governing text is `000_op.md:69` plus the four sections above. Every build step below was checked against them. Section 1 lists what op's second answers make wrong in 014. I am the third reader of canon fit, after seat 001 (015) and seat 002 (016). Every claim in the ledger that the canon permits a thing is held by at least two seats.

Test gate. The whole suite, as run for the audit (`010_probes/suite_partial_output.txt`, completed run reported in 014): 3571 passed, 3 failed, 87 ignored. The three red tests:
- `tests/refs_consumer_setups.rs:168`;
- `tests/refs_daily_history.rs:105`;
- `tests/refs_cost_scaling.rs:160`.

The suite defects from 014 stand:
- `tests/refs_backup.rs:325-327` returns green without running;
- `tests/refs_consumer_migrations.rs:171-183` pins bytes where op asked for the pointer;
- `tests/refs_daily_history.rs:106-111` samples four rows.

No code changed in this round.

Evidence. Every probe I cite is committed beside its file. They are git 2.43.0 spikes on Linux over `file://` remotes; none is a bench. The renderers in `015_probes/j1`, `016_probes/p1` and `017_probes/k1`/`k2` are toys. They show git's side of the two-way machinery, not mockspace's renderer. No seat tested a real forge, Windows, macOS or reftable. This round's probes ran under `timeout`, with stdin from `/dev/null`, each in its own scratch directory, which they refuse to run outside of. Nothing was killed.

## 0. Op's second answers, verbatim, and the intent of each

Q1, the instance root:

> We should just go with the platform compatible .mock and not try to be fancy with the dirname that’s bound to cause extra work and edge cases we didn’t think of. Also does it have to be in the actual same dir as the canonical data? Could it just be located in .local/state or such whatever platform intended place is for that kind of thing? The commands just work in the git tree too and associate it. Or I don’t know, just a thought, maybe not good

Intent: the instance root is `.mock`, and `git~1` is dropped. The state directory is a thought, not a ruling. Section 2.4 (D11) weighs it, and the evidence settles it, so it is not handed back as a question.

Q2, the shadow tree:

> Wait if @ is already something else then let’s choose something else. Yeah one shadow tree to the git tree, anything in there is always properly processed and the way it works should be ergonomic enough that no trivial merge conflicts come from regenerations or such and instead it would take the diff of the latest generated end result vs current and upstream those merge conflicts to the template. This way regular PRs in gh or such can contain doc changes and they can get through, then whoever next works on mockspace, upstream easily those changes to templates. So it’s two way

Intent:
- There is one shadow tree for the git tree, under a name git gives no meaning; `@` is out.
- Every file in the shadow tree is processed.
- Regeneration causes no trivial merge conflicts.
- An edit made to generated output in an ordinary pull request survives the merge and is carried back into the template.

Q3: auto-install git-shook. Intent: git-shook is installed by the engine, not demanded of the person.

Q5: purge epoch. Intent: a deliberate purge goes through an epoch that every clone honours.

Q4 (a second remote by default) was not answered. It stays open, as Q-A in section 5.

## 1. What op's second answers make wrong in 014

Each item below is withdrawn or rewritten here. 014 stays in the trail as history.

- L19, L20 (git refuses `git~1`; no pollution can be committed under a `git~1` root). The facts remain true, but they no longer bear on the build: the root is `.mock`. The consequence is serious and is stated plainly. With an ordinary name, a commit that tracks a file under `.mock/` can be made, and moving onto such a commit destroys an instance edit. 014's R2 used to apply only to the fallback. It is now the default row, and its bounds are rebuilt in section 3.5 (R2).
- D5 (root `git~1` after an identity check against `.git`). Superseded. The identity check, the `core.protectNTFS` check in `verify`, and the `git~1` arms of the test matrix are all deleted.
- D6 (a physical `@`). Superseded by op. There is no marker directory at all (D9).
- Glossary items "Instance root" and "Shadow tree, `@`". Rewritten in section 2.
- 014 2.2 (root choice) and 2.3 (`@`, φ across namespaces, "`@/` permitted in any"). Rewritten in 3.2 and 3.3. There is one shadow tree, not one per namespace.
- 014 Q1, Q2, Q3, Q5. Answered. Q4 stays open.

One fact has to be carried forward without change. The `fsmonitor-watchman` route through git-shook still hangs, and still swallows a calling script's input, for any git command run with a non-terminal stdin (`014_probes/g1b`, `012_probes/f1_hang_evidence.txt`). Auto-installing git-shook (op's Q3) means installing that defect into every opted-in clone. The fix therefore has to land before auto-install ships (3.7).

## 2. Glossary

- Git tree: the code tree, meaning the content of a worktree's code branch, plus its untracked and ignored files. Its refs are under `refs/heads/`.
- Mock tree: one namespace of mock data, on its own ref outside `refs/heads/` and `refs/tags/`, with its own history and never part of the git tree's history. "The mock trees" means all of them together.
- Kind: what a mock tree holds, and the rule its commits follow. There are four kinds:
  - `data`: materialised as an instance;
  - `shadow`: the shadow tree;
  - `registry`: the registry tree, which follows the store's merge hook;
  - `history`: the test history.
- Shadow tree: the single mock tree of kind `shadow`. Its root corresponds to the git tree's root. Its namespace name is op's to choose (Q-B). Until he does, it is written `<shadow>`.
- Namespace, `<ns>`: a mock tree's name. Namespaces form a prefix-free set (L13).
- `mock://<ns>/<p>`: the engine's address of a path in a mock tree. It resolves two ways:
  - working: to `<worktree>/.mock/<ns>/<p>`;
  - committed: to the blob `<tip>:<p>`.
  It is never handed to git, because git routes `mock://` to a remote helper (`013_probes/p4`).
- Instance root: `.mock`, one component directly under each worktree's root, ignored and never tracked. Its leading dot makes it unusable as a ref name, so `git log .mock` is never ambiguous (`015_probes/d1`).
- Instance: a linked worktree that materialises one mock tree at `.mock/<ns>`, one per git-tree worktree that opted in. Its HEAD is a symref onto the working ref. An instance whose namespace is declared position-independent may live outside the tree instead (D11).
- Published ref: `refs/mock/<ns>` on the remote, in the same repository by default (op).
- Opted-in clone: a clone with a dedicated remote `mock-<ns>` for each mock tree it asked for. A plain clone has none of these remotes and fetches no mock tree.
- Mirror ref: `refs/mock/<ns>` in an opted-in clone. It is fetched unforced, so it is monotone: it records the highest tip the clone has seen (L22).
- Working ref: `refs/worktree/mock/<ns>`, a per-worktree ref (L11).
- Snapshot ref: `refs/mock-snapshot/<enc ns>@<enc wt>`, the ref-held recovery copy of an instance.
- User cache: `${XDG_CACHE_HOME:-$HOME/.cache}/mockspace/<clone id>/<ns>/<wt>/<stamp>/`, kept for 24 hours, with the newest entry always kept.
- Template: a file of the shadow tree whose name ends in `.tmpl`. It shadows the git-tree file at the same path with that suffix removed (`crates/foo/README.md.tmpl` shadows `crates/foo/README.md`).
- Generated file, G: a git-tree file the engine writes from its template T, the data named by the landing's `Mock-Ref`, and the git-tree context C. `Gen(T, C)` is required to be deterministic.
- Produced set of G: every blob id the engine has ever written for G. It is kept in the shadow tree (D10).
- Edited G: a G whose blob at HEAD is not in its produced set.
- Carry back: turning an edit of G into a change of T (3.3).
- Epoch: an integer stored in the file `.epoch` at a mock tree's root. When the file is absent the epoch is 0. Tips are ordered first by epoch, then by ancestry (L32).
- Pre-write copy: the snapshot taken by the `fsmonitor-watchman` hook before git reads the index.
- Repair: the engine's automatic, unforced re-publication of a published ref that the remote has lost.

## 3. Ledger

The holder codes:
- 001 means seat 001 (files 001, 003, 005, 008, 012, 015);
- 002 means seat 002 (files 002, 004, 006, 009, 013, 016);
- 007 means my files (007, 010, 014, 017).

### 3.1 Carried from 014, unchanged

Each row below has the holders, probes and limits listed for it in `014_consolidation.md` 1.1. With the root now `.mock`, it holds exactly as stated there.

- L1: a native worktree, merged by git.
- L2: old commits never reach a path that no commit has tracked.
- L3: depth, so the root is one component (`012_probes/u1` part c builds with `.mock`).
- L4: never unhidden.
- L6: a tracked ignore line plus `info/exclude`.
- L7: `update` is fetch, then `merge --no-overwrite-ignore`.
- L8: locks.
- L9: double-f `clean`.
- L10: `refs/mock/<ns>` on the same repository.
- L11: the working ref `refs/worktree/mock/<ns>`, still owed a second read (P6).
- L12: mirror push damage.
- L13: prefix-free namespaces.
- L14: the copied index.
- L15: the hook runs before the destructive verbs.
- L16: a second repository is optional.
- L17: pairing.
- L18: no `core.bare` edit.
- L21: what a `pre-push` hook sees.
- L22: unforced explicit fetch.
- L23: unforced repair never lowers the remote.
- L24: objects survive a deletion until the remote's gc.
- L25: the cost of the pre-write copy.
- L26: the git-shook stdin defect.
- L28: `mock://` collides with git's helper dispatch.

Two rows change:
- L5, pollution. The finding stands. Since `git~1` is dropped, the prevention half no longer applies, so the row now reads: "a commit from any clone that tracks a file under the instance root destroys an edit when a clone moves onto it, and with a hidden root even plain `checkout` and `merge` do it". Held by 001, 002 and 007. Probes: `007_probes/a1`, `a2`; `008_probes/t2`; `009_probes/p1`.
- L27, the `@` collision. It is history, now that the name is out.

### 3.2 New rows

- L29. Names that can be refs collide on purpose. Any directory name that can also be a ref becomes ambiguous to `log`, `diff` and `show` the moment a ref of that name exists, and `checkout <name>` then switches branch instead of restoring the path. That holds for `@`, `shadow`, `_shadow`, `#shadow`, `%shadow`, `=shadow`, `+shadow` and `$shadow`. Names that start with a dot, and names that end in `.lock`, can never be refs and are clear on all eight verbs tested. Most punctuation-led names also break in the shell. Held by 001 and 007; 002 agrees on `@` (`013_probes/p4`). Probes: `015_probes/d1`, `014_probes/g3`.
- L30. Identical regenerations merge cleanly in plain git. A pull request's literal edit merged against a regeneration keeps both. An edit to a computed line against a regeneration of that same line conflicts, and the pull request's text stays visible. Held by 002 and 001. Probes: `016_probes/p4`, `015_probes/j1` (two different regenerations give `UU`).
- L31. Carry back by `git merge-file` plus a round-trip check absorbs edits to literal lines, and refuses edits on or next to computed lines, overlapping edits, and empty files. Held by 001 and 002. Probes: `015_probes/j1` (11 cases), `016_probes/p1` (12 cases).
  - The base must belong to the same line of history as the output it is compared against. 001's run shows the wrong pairing (the latest G as base against the pull request's tip) reading the pull request as reverting a template change made upstream (`015_probes/j1`, last block). The base disagreement is settled in D10.
- L32. A purge epoch with the extended order:
  - lets a purge through the hook, and refuses a rewind within the same epoch;
  - makes a stale clone adopt the purge, rather than republish, as soon as it touches the remote or receives the epoch through `Mock-Ref` lines;
  - carries local-only commits forward;
  - leaves exactly one case where a purge is undone: a clone that has neither touched the remote nor seen the epoch since the purge, and that creates the ref while it is absent. A later clone holding the higher epoch reverses that.
  Held by 002, with 007 agreeing; 001 has no position. Probe: `016_probes/p2`. A second read is owed (P7).
- L33. Position-independent mock trees can live outside the tree. Placing an instance outside the git tree:
  - removes the double-f `clean` and `rm -rf` losses for that instance;
  - keeps the bytes if the clone itself is deleted, while the commits go with it;
  - breaks every relative path that climbs out of the namespace when a tool is run with its working directory inside the instance.
  Held by 001 and 002, with 007 agreeing. Probes: `015_probes/s1`, `016_probes/p3`; the `..` wall itself is in `003_probes/q2`, `005_probes/r1` and `006_probes/p1`.
- L34. The shipped `mockspace-generated` driver keeps ours on every generated file whenever it judges that a regeneration follows (`src/bootstrap/driver_script.rs:18-37`, and the `generated && followed_up` block at `:112-:125`). Only `Cargo.lock` gets a text merge first. Any other file keeps ours whole, so a pull request's edit is dropped from the merge result. The edit is not dropped only from the conflicting hunk, as the ticket `the_mockspace_generated_merge_driver_keeps_ours_and_drops_a` says: it is dropped in full, whenever git calls the driver. Git calls it whenever both sides changed the file. The edit stays in the pull request's commits, so it is a silent revert, not a lost byte. Held by 002 (from reading the source) and 007 (source, plus the keep-ours model in `017_probes/k2`, rows marked "shipped"). The real driver was not run.

### 3.3 Disagreements, settled

D9. The shadow tree's form: a namespace of its own, or a marker directory.
- 001 wants a namespace of its own, with no marker, and `.shadow` as the marker only if shadow files ever have to share a namespace with other files (`015:50-52`).
- 002 wants a namespace of kind `shadow`, with its root standing for the git tree's root (`016:13`).
- The two agree on the namespace form.
- Op asked for one shadow tree for the git tree, under a name git gives no meaning. In the namespace form the shadow name appears only as a path component under `.mock/`, which git never parses as a revision (L29), as a ref component, and inside `mock://`. No directory at the top of a worktree can be mistaken for a revision.
- Settled: the shadow tree is a namespace of its own, with no marker directory. The `.shadow` fallback is not built, because a second shadow tree contradicts op's "one shadow tree".
- The namespace name is op's (Q-B). It must pass the existing validator, and it should avoid names git gives meaning to (`HEAD`, `FETCH_HEAD`, `ORIG_HEAD`, `MERGE_HEAD`, `-`). In the namespace form even those names are harmless as paths, because they sit under `.mock/`.

D10. The carry-back base: G at the pull request's merge-base, or a stored baseline B.
- 001 takes G at the merge-base, checked by re-rendering T as it was then (`015:66`).
- 002 stores one baseline B per output in the shadow tree, the last bytes the engine wrote (`016:23`).
- Evidence against B. B is one value per repository, but outputs live on many branches of the git tree. In `017_probes/k1`, a feature branch that regenerated from newer data is read as edited by a single stored B. After that branch's write, trunk's real carry back hits a false conflict on the computed line (`merge-file rc=1; round trip: no`), which is the trivial conflict op ruled out. B's strength is that it survives a generator upgrade without any re-rendering.
- Evidence against the merge-base form. It needs the pull request identified and T as it was re-rendered with the data and generator of that time. That re-render fails exactly where 002 says, on a generator upgrade, and it has no answer for an edit made without a pull request (a direct commit, a squash, a cherry-pick).
- Settled, composing the two halves that hold. The shadow tree stores, per output, the set of blob ids the engine has ever produced; this is 002's record, made a set. The base is G at the newest commit on HEAD's first-parent chain whose G is in the set; this is 001's history-relative base, found without identifying any pull request and without re-rendering. `017_probes/k1` covers both sides:
  - the regenerated feature branch is classed not-edited;
  - trunk after the pull request merges is classed edited;
  - the base is trunk's own template-change regeneration;
  - `merge-file` is clean, the round trip holds, and T' carries both the pull request's line and the upstream template change.
  The set is append-only and survives generator upgrades. A blob that coincides with an engine output is engine output by definition, so it is harmless. The generator version never enters the check.

D11. Placing an instance in the state directory, as a per-namespace declared property.
- 001: the default is `.mock` in the tree. A namespace may declare an external root, and `instantiate` refuses it when a scan finds a relative path leaving the namespace, or when the namespace is read by path. 001 also prefers `XDG_DATA_HOME` over `XDG_STATE_HOME`, because data a person made belongs there (`015:27`).
- 002: a declared property ("position-independent"), checked by a scan at `instantiate`, with the clone's key stored in the clone's git directory so that the key survives a move (`016:135-138`).
- They agree on the shape, and the evidence (L33) decides it. The research tree here cannot move out: 13 of 24 manifests climb to the root (`010_probes/c3`).
- Settled:
  - the property is `placement = "tree" | "external"`, default `tree`;
  - external means `${XDG_DATA_HOME:-$HOME/.local/share}/mockspace/<clone id>/<wt>/<ns>`, with each platform's data directory (impression for macOS and Windows, P5);
  - `<clone id>` is a random id written once to `$(git rev-parse --git-common-dir)/mockspace/id` (002's form, which survives `mv`);
  - an excluded symlink `.mock/<ns>` points at the external instance, so editors and searches still find it, and when that link is cut by `clean` only the link is lost;
  - the declaration is the authority, and the scan is a refusal check for the forms it can read (Cargo path dependencies, Markdown relative links, `include_str!`/`include_bytes!`).
  The shadow tree resolves its relative links in git-tree space by construction (`016:15`), so it may be declared external. The default stays `tree`. Op's thought is answered: yes, per namespace, for trees that are position-independent, and never for research. Nothing is owed back to him.

D12. The generated-file merge driver. This is a finding (L34) and a change to a shipped mechanism, so it is a design change.
- 002 asks for the conflicting hunk to be kept, or for theirs to be kept (`016:78`).
- Settled by `017_probes/k2`. Keep ours, and record that a regeneration is owed, only when both ours and theirs are in the produced set (two pure regenerations, op's "no trivial merge conflicts").
- Otherwise run `git merge-file` and return its status. A literal edit then merges in beside the regeneration (k2: `Intro line, by a PR.` survives), and an edit to a computed line becomes git's own visible conflict (k2: markers, rc 1). Keep-ours dropped both (k2 "shipped" rows).
- A binary file returns a conflict and leaves ours in the worktree, with theirs at index stage 3.
- `Cargo.lock` keeps its shipped text merge.
- A clone without the shadow tree cannot read the set, so it falls through to `merge-file`, which is git's default. GitHub, where no custom driver runs, behaves like that too (L30), so pull requests "get through" exactly where a human edit and a regeneration do not overlap.

D2, D3, D4 and D8 of 014: D2 is resolved by D11 (external placement replaces the common-dir arm). D3 was superseded in 014. D4 is still owed its second read (P6). D8 stands, and its gates are carried into 3.7.

## 4. The build

### 4.1 Refs

These are as 014 2.1, unchanged except that every kind uses them.

On the remote:
- published refs are `refs/mock/<ns>`, in the same repository by default;
- the URL is the code remote's, unless the namespace declares another.

Per opted-in clone and per namespace, a remote `mock-<ns>` with:
- `fetch = refs/mock/<ns>:refs/mock/<ns>`, explicit and unforced;
- `push = HEAD:refs/mock/<ns>`;
- `tagOpt = --no-tags`.

Local refs:
- the mirror ref is `refs/mock/<ns>`;
- the working ref is `refs/worktree/mock/<ns>`, created with `update-ref --create-reflog`, with the instance's HEAD a symref onto it;
- the snapshot ref is `refs/mock-snapshot/<enc ns>@<enc wt>`;
- `refs/mock-backup/*` is kept, never rewritten;
- no mock ref is ever under `refs/heads/`, `refs/tags/` or `refs/remotes/`;
- namespaces are prefix-free.

### 4.2 Placement

- Instance root: `.mock`, one component, at every worktree's root.
- Ignoring: a tracked `/.mock/` line in the git tree's `.gitignore`, which covers fresh clones that never ran the engine, plus the same line in `info/exclude`, which covers pre-migration commits.
- An instance of `<ns>` sits at `.mock/<ns>`, or, for an external namespace, behind the `.mock/<ns>` symlink (D11). There is one instance per git-tree worktree per namespace that has an instance.
- Locks:
  - every instance is locked with the reason `mockspace instance <ns>`;
  - a linked parent worktree is locked with the reason `holds mockspace instance <ns>`;
  - only `retire` unlocks, and it takes a snapshot first.
- `registry` and `history` trees have no instance. The engine reads them by their committed contents.

### 4.3 The shadow tree, two-way

Contents. Every file in `mock://<shadow>/` is exactly one of:
- a template, `*.tmpl`, shadowing the git-tree path with the suffix removed;
- an engine record under `.mock-gen/`;
- the tree's `.epoch`;
- its `.gitattributes`.

`verify` and a lint refuse anything else, which is how op's "anything in there is always properly processed" holds. Two templates may not shadow one git path; the mapping is the identity on the directory part, so this cannot happen except through a `.tmpl.tmpl` file, which is refused. Generated output is the only place that names git-tree paths. Nothing in the git tree names a mock path physically; links use `mock://`.

Records. `.mock-gen/<git path>.produced` holds one blob id per line. The shadow tree's `.gitattributes` gives `.mock-gen/** merge=union`, so the set merges across clones by git (duplicates are harmless, and the engine dedups when it writes). Each engine write of G appends `git hash-object G`.

`regenerate`, per generated output q, run by the engine and by the shipped post-merge regeneration:
1. Render `Gen(T, C)` twice. If the results differ, refuse q and name the generator.
2. Let O be the blob of q at HEAD.
3. If O is in `produced(q)` (not edited):
   1. write `Gen(T, C)` to q;
   2. append its blob id;
   3. done.
4. Otherwise (edited), carry back:
   1. The base is G at the first commit in `git rev-list --first-parent HEAD -- q` whose `<c>:q` blob is in `produced(q)`. If there is none, q is in conflict state with the reason "no baseline", and nothing is written.
   2. `T' = git merge-file -p T base O`.
   3. `O' = git merge-file -p Gen(T, C) base O`.
   4. Accept if and only if both merges are clean and `Gen(T', C) == O'`.
   5. On accept: commit T' in the shadow instance, with the message `carry back <git-tree sha> <q>`, write `Gen(T', C)` to q, and append its blob id.
   6. On anything else: write nothing for q, and record the conflict (q, the hunk, and its class from 4.3.1) for `status`.
5. Never overwrite O while q is edited and not carried back. This is invariant N1 of `016:48`, and it is the rule that protects the edit across every regeneration until a person settles it.

4.3.1 Where carry back must be done by a person (L31). In each of these cases `status` names q, the hunk and, where the renderer can say, the source of the computed value:
- the edit is on, or next to, a computed line (a placeholder, a loop, a conditional, an include);
- the edit overlaps a template change;
- the merge is clean but the round trip fails (a construct that does not align by line);
- there is no baseline;
- the generator is not deterministic;
- the output is not line text;
- the output comes from several templates and the hunk's owner is ambiguous;
- the pull request deletes, empties or renames q (a decision about the template);
- a new hand-written git-tree file has no template. The person either adopts it (the engine writes it out as a literal template and records its blob) or declares it hand-written.

The gate reports pending carry backs and does not refuse a landing for them, because op wants pull requests to get through. The edit is protected by step 5.

The merge driver is D12. It replaces the `generated && followed_up` block of `src/bootstrap/driver_script.rs`, and the agent text at `src/render_agent/templates.rs:266` ("keeps this side's copy") is rewritten to match.

### 4.4 Engine verbs as git commands

Notation, as in 014 2.4: D is the instance path, W = `refs/worktree/mock/<ns>`, M = `refs/mock/<ns>`, X = `mock-<ns>`. "Snapshot" means 4.5. "Contact" means 4.6.

`instantiate <ns>`:
1. Refuse while a merge, cherry-pick or rebase is in progress in the code worktree.
2. Ensure git-shook is installed (4.7).
3. Configure X.
4. Run `git fetch X`. If the remote ref is missing: for a namespace that has never been published, the tree is unborn; otherwise run contact.
5. Choose D by the placement property. For `external`, first run the scan, write `<clone id>` if it is missing, and create the symlink.
6. Move any existing D that is not an instance into the user cache.
7. `git worktree add --detach D M`.
8. `git -C D update-ref --create-reflog W M`.
9. `git -C D symbolic-ref HEAD W`.
10. Add the ignore lines.
11. Lock.
12. Snapshot.

`status`:
- `git -C D status --porcelain=v2 --branch`;
- `rev-list --left-right --count W...M`;
- the repair state;
- pending carry backs.

`update`:
1. Snapshot.
2. Contact.
3. List the incoming paths with `git -C D diff --name-only W M`; the ignored files at those paths go into the snapshot and the cache.
4. `git -C D merge --no-edit --no-overwrite-ignore M`.

`commit`:
1. Snapshot.
2. Lints run as git-shook's `pre-commit` entry, which recognises an instance by its symref.
3. `git -C D add -A`.
4. `git -C D commit`.

This removes `--no-filters` at `mock/crates/mockspace-store/src/instance/files.rs:228`, `instance/backup.rs:320` and `ref_ops.rs:303`.

`push`:
1. Contact.
2. `git -C D push X HEAD:M`, unforced.
3. If rejected: `update`, then one retry.

`restore`: snapshot, then `git -C D restore --source=<snapshot> --worktree -- <paths>`.

`retire`:
1. Refuse unless D is clean and W is an ancestor of M.
2. Snapshot.
3. Unlock.
4. `git worktree remove D`.
5. For an external namespace, remove the symlink.

`regenerate`: as 4.3.

`purge`: as 4.6.

`verify`, read-only:
- the locks are in place;
- HEAD is the symref onto W;
- `git ls-files -- .mock` is empty in every worktree of the git tree;
- the ignore lines are present;
- X is configured as above;
- no mock ref is under `refs/heads/`, checked locally and through `ls-remote`;
- namespaces are prefix-free;
- the shadow tree's contents follow the rule in 4.3;
- the produced sets are well-formed;
- the epoch witness is consistent;
- for external namespaces, the scan passes and the symlink resolves.

`verify --fix` repairs worktrees and re-adds lines. It never deletes anything.

### 4.5 Snapshot, user cache, pre-write copy

These are unchanged from 014 2.5:
- the snapshot is a copied index, `add -A`, plus the ignored files at risk added with `-f`, then `write-tree`, then a commit only if the tree changed, then `update-ref --create-reflog` on the snapshot ref;
- the user cache holds plain copies plus a `MANIFEST`, and is pruned to 24 hours at engine start, always keeping the newest entry;
- the pre-write copy is git-shook's `fsmonitor-watchman` entry. It prints nothing, snapshots each instance of the worktree when its tree id has changed, carries a re-entry guard, and leaves the protocol answer to the person's own entry or to git-shook's floor.

At 30 000 files it costs roughly 60 to 110 ms per command over the floor, with nothing running between commands (`014_probes/g2`; an ad-hoc spike, not a bench).

### 4.6 Mirror guard, repair, purge epoch

The epoch:
- `epoch(t)` is the integer in `<t>:.epoch`, or 0 when the file is absent. Because it is part of the tree, every vanilla commit inherits it.
- Order: a is at least b if and only if epoch(a) > epoch(b), or the epochs are equal and b is an ancestor of a (or equal to it).
- Witness `n_known`: the largest of the epoch of M and the epochs named by the `Mock-Ref: <ns> <sha> epoch=<n>` lines that the clone has fetched in the git tree. A line without `epoch=` means 0.

Layer 1, git-shook's `pre-push` entry, refuses:
- any delete of `refs/mock/*`;
- any update of `refs/mock/*` that is not at least the remote tip in the order;
- any push carrying a ref name that only `--mirror` sends (`refs/mock-snapshot/*`, `refs/remotes/*`).

It cannot see the deletions a mirror push computes, it is absent in plain clones, and `--no-verify` skips it (L21).

Layer 2, contact, runs on every engine command that talks to the remote. It takes r = `git ls-remote X M` (fetching r's commit if epochs have to be compared), s = M, and n = max(epoch(s), n_known), then:
1. r is absent and s exists: if epoch(s) is at least n, run `git push X s:M` unforced; otherwise refuse, and adopt on the next contact.
2. epoch(r) > epoch(s): adopt.
   1. `git fetch X +M:M`. This is the only forced fetch, and it is allowed only in this case.
   2. Cherry-pick the commits that are reachable from W but not from the old s onto r, under a temporary local ref. A conflicting pick stays in `refs/worktree/mock/<ns>-pre-epoch-<n>` and is never pushed.
   3. Delete the snapshot refs and expire the cache entries of lower epoch.
   4. `git reflog expire --expire-unreachable=now --all`.
   5. `git gc --prune=now`.
3. r is a proper ancestor of s within one epoch: run `git push X s:M` unforced. This is a fast-forward.
4. r is at least s: nothing to push; fetch as usual.
5. Anything else: nothing automatic; `status` reports the divergence, and `update` merges it.

Repairs are counted per 24 hours, and past a threshold the engine stops and warns (L23, `013_probes/p2`, `014_probes/g4`).

Layer 3 is memory: the mirror ref, the working ref, the snapshot ref, the user cache, every `clone --mirror`, and the `Mock-Ref` ids.

`purge <ns> <paths or filter>`:
1. Refuse unless the authority of Q-C holds.
2. Snapshot.
3. Build the new tree, which is the tip's tree minus the purged content, with `.epoch` set to epoch(r)+1.
4. `C = git commit-tree <tree> -m "purge epoch <n+1>"`, with no parents.
5. `git push X --force-with-lease=M:<r> C:M`. This is the engine's only forced push.
6. Locally, run step 2 of layer 2 as the purging clone.
7. Land a git-tree commit whose `Mock-Ref` line carries `epoch=<n+1>`, so that the witness reaches clones through ordinary code fetches.

Old `Mock-Ref` ids of a lower epoch become historical. The pairing gate checks only lines of the current epoch.

What a purge cannot remove: the remote's objects until the host runs gc, any `clone --mirror`, and any clone that never makes contact again (L32). Op should know that a purge also empties the recovery cache of that history, by design.

### 4.7 git-shook, auto-installed

`instantiate` installs git-shook if it is absent: `git-shook install --yes` in the repository (the command used by `012_probes/f1`). The engine then registers four entries:
- `pre-commit`: lints;
- `pre-push`: layer 1;
- `fsmonitor-watchman`: the pre-write copy;
- `post-merge`/`post-rewrite`: regeneration.

Two gates on shipping, carried from 014 D8, because auto-install spreads the defect to every opted-in clone:
- git-shook replays stdin only for the events git feeds on stdin (`pre-push`, `pre-receive`, `post-receive`, `post-rewrite`, `reference-transaction`, `proc-receive`, `push-to-checkout`);
- only one entry answers the `fsmonitor-watchman` protocol, the person's own or the floor.

Before either gate is met, auto-install installs every entry except `fsmonitor-watchman`, and `status` says the pre-write copy is off and why. That a person's existing hooks survive the install is unprobed (P3).

### 4.8 to 4.10 Migrations and test matrix

The migration of data trees (4.8), of the registry stores (4.9) and the test matrix (4.10) are in `017_consolidation_migration_and_tests.md`, split along that seam to keep each file under 500 lines. They are part of this candidate.

## 5. Residual loss paths

R1. In the git tree, the following destroy uncommitted instance edits made since the last copy:
- `clean -ffdX` or `-ffdx`;
- `rm -rf`;
- `worktree remove -f -f`.

Bounded by the newest of the snapshot, the pre-write copy (once 4.7's gates are met) and the user cache. An external instance (D11) is outside R1 entirely.

R2. Now the default. A commit tracking a file under `.mock/` destroys an edit when a clone moves onto it, silently by plain verbs (L5). It is bounded:
- by prevention in opted-in clones: git-shook's `pre-commit` and `pre-push` refuse such a commit;
- by the accident barrier for fresh clones: the tracked ignore line makes `git add -A` skip the path, leaving only `add -f` or plumbing;
- by detection: `verify` and a CI lint;
- by the pre-write copy, which runs before the destroying `checkout` or `merge` (`012_probes/f2`).

What it is not bounded by is git itself. That was the price of dropping `git~1`, and op chose it knowingly.

R3. In the instance, git's own destroyers (`reset --hard`, `checkout -f`, `clean -ff*`). Bounded by R1's copies and the reflogs.

R4. On the remote, a mirror, forced or deleting push from a clone without a hook. Bounded by layers 2 and 3. It becomes a loss only through two failures together: the host collects the objects, and every holder is gone.

R5. HEAD leaves the working ref. Nothing is lost; `verify` names the state.

R6. In a mirror clone, the snapshot ref is pruned by a pruning fetch. Bounded by the user cache and the working ref.

R7. Deleting the clone loses what was neither pushed nor cached. An external instance keeps its uncommitted bytes.

R8. A registry row held in one clone that dies during R.0 to R.5. Bounded by the bundle.

R9. A purge removes its objects by design, from every clone that makes contact and from the caches. That is the one deliberate exception to "no loss". A clone with no contact and no witness can republish while the ref is absent, until a clone holding the higher epoch touches the remote.

R10. A human edit in G that is pending carry back is overwritten by a regeneration from a clone that runs an older engine, one that predates the produced set and the D12 driver. Bounded: the edit stays in git-tree history, and the base walk of 4.3 recovers it on the next new-engine regeneration. The step-5 protection cannot hold while old engines remain.

## 6. Questions still op's

Q-A, a second remote by default (014 Q4, unanswered):
- (a) Off. Same-repository refs only, as op ruled for the default.
- (b) On where declared. This adds a failure domain that a mirror push to `origin` cannot reach, with no server configuration. The cost is a second repository and its access control, kept in step.

Q-B, the shadow tree's namespace name. Both seats leave it to op. Constraints: it must pass the validator, and it should avoid names git gives meaning to.
- (a) `shadow`: the panel's working word, and it reads as what it is.
- (b) `git`: it says "the git tree's mirror", but it reads like a reference to git itself in `mock://git/...` and `.mock/git`.
- (c) A name of op's own.

Each costs the same; the choice is about how it reads.

Q-C, who may publish a purge (L32 makes it possible, but nothing yet decides who may):
- (a) Any opted-in clone, through `purge` with an interactive confirmation. Cheapest. A careless or compromised clone can purge.
- (b) Only a clone whose purge commit is signed by a key declared in the git tree, checked by every clone's contact. Costs key management, and a purge then fails for anyone without the key.
- (c) Only through a server-side update hook. Self-hosted remotes only; on a forge it is impossible.

## 7. Probes owed, ranked by what each would change

- P1. A real host, for each host op uses: whether it accepts `refs/mock/*`; what `push --mirror` does; fetch by id after a deletion; how long unreachable objects survive; whether it refuses non-fast-forwards (which would stop a purge). Decides whether the default vehicle works, and whether the purge is possible.
- P2. The git-shook fixes (stdin, the single answering entry), then `014_probes/g1b` and `012_probes/f1` re-run. This is the gate on the pre-write copy and on safe auto-install.
- P3. git-shook auto-install over a person's existing hooks, `core.hooksPath` and an existing `core.fsmonitor`. Decides whether the install can be silent.
- P4. The real renderer under carry back: line alignment, a formatter, includes, a source map, determinism checked twice. Decides how much of 4.3.1 is common in practice. The toys prove git's side only.
- P5. External placement on macOS and Windows data directories, and the `.mock/<ns>` symlink on Windows. Decides D11 outside Linux.
- P6. Second reads of the per-worktree working ref (L11), plus its rebase, merge and gc reachability once HEAD has moved off it.
- P7. A second read of the purge epoch (L32) by a seat other than 002, with the witness carried only through `Mock-Ref` lines.
- P8. The shipped engine's behaviour when its remote branch disappears or reappears. Decides how the R.4/R.5 sweep is timed.
- P9. A harness bench of the pre-write copy at 30 000 and 100 000 files, with every arm. Prices op's sub-second bound.
- P10. A census of which namespaces any tool reads by path, and of relative paths in templates. Decides which namespaces may be external, and whether carry back has to rewrite paths.
- P11. `adopt` of a pre-migration feature branch through `git apply -3`.
- P12. Reftable on git 2.46 or later. P13. LFS through a real server, with the per-namespace remote.
