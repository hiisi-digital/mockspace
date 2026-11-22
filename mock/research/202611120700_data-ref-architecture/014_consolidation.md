# Mock trees: consolidation after op's answers (seat 003, Fabian Giesen)

Status: this file replaces `010_consolidation.md` as the canon candidate for the design rounds. It is unratified. Op's words in `000_op.md` govern: the sections "Answers to the consolidation" (lines 89-115) and "The registry stores" (lines 117-123). Where a seat's file and this one disagree, both are cited so op can read the evidence directly.

Gates.

Canon gate. No `canon_paths` is declared. What governs is `000_op.md:69`, his ruling during the audit, and the two sections above. Each ruling is built in, and section 1.3 lists them with the place each lands. The disagreements this file settles are ones op's words leave open, which is why I settle them by probe rather than by reading his words. Seats 001 (012) and 002 (013) each gave a reading of canon fit. This file is a third, and every reading that the canon permits something is held by at least two seats in the ledger.

Test gate. My whole-suite run of the audit finished: 3571 passed, 3 failed, 87 ignored, git-shook on PATH. The three red tests are:
- the bare clone test, `tests/refs_consumer_setups.rs:168`;
- the forced way back, `tests/refs_daily_history.rs:105`;
- cost scaling, `tests/refs_cost_scaling.rs:160`.

The suite defects named in 001 and 007 stand:
- the reftable test returns green without running, at `tests/refs_backup.rs:325-327`;
- the LFS test pins bytes, at `tests/refs_consumer_migrations.rs:171-183`;
- the old-commit law is sampled at four rows, at `tests/refs_daily_history.rs:106-111`.

Disclosure. One probe of this round is still running in the background: my first g1 run, task id `bh0pg0pc3`. It is blocked on the defect it was written to find (section 1.2, D8). I was told to kill nothing, and I do not have a stop tool. It ends at the harness limit, and `014_probes/g1_hang_evidence.txt` records its process tree. Seat 001's f1 run is in the same state, recorded at `012_probes/f1_hang_evidence.txt`.

Evidence. Every probe cited is committed beside the file that cites it. They are git 2.43.0 spikes on Linux over `file://` remotes. g2 is a timing taken outside the harness and is labelled as such in its header. It is an ad-hoc quick spike, not a bench. No real forge, Windows, macOS, NTFS, WSL or reftable was tested by any seat.

## 0. Glossary

These are the terms op asked to be formalised (`000_op.md`, Q2 answer). They are the vocabulary of every design round that follows.

- Git tree. The code tree: the content of a worktree's code branch at the worktree root, plus its untracked and ignored files. Its refs are the refs under `refs/heads/`.
- Mock tree. One namespace of mock data, carried on its own ref outside `refs/heads/` and outside `refs/tags/`, with its own history. Together, "the mock trees". A mock tree is never part of the git tree's history.
- Kind. What a mock tree holds and which merge rule its commits follow. There are three kinds:
  - `data`, which is materialised as an instance;
  - `registry`, the registry tree, which keeps the store's own merge hook;
  - `history`, the test history.
  A kind with a bespoke name keeps it, which is how the registry tree gets its name.
- Namespace, `<ns>`. The name of a mock tree. Namespaces form a prefix-free set: no namespace is a path prefix of another (L13).
- Published ref. `refs/mock/<ns>` on the remote: the one ref a mock tree has on a remote.
- Opted-in clone. A clone that has asked for one or more mock trees, by having a dedicated remote `mock-<ns>` for each one. A plain clone has none of them, and fetches no mock tree (L10).
- Mirror ref. `refs/mock/<ns>` in an opted-in clone. It is fetched unforced, so it only ever moves forward and is the clone's monotone record of the highest published tip it has seen (L22, D7).
- Instance root, ρ. One configured path component directly under the worktree root, never tracked and always ignored. It is not called `mock` and has no default literal `mock/` (op, Q1 answer). Its default name is decided in D5.
- Instance. A linked worktree of the repository that materialises a `data` mock tree at `<ρ>/<ns>`, one per git-tree worktree that opted in. Its HEAD is a symref onto the working ref.
- Working ref. `refs/worktree/mock/<ns>`, which is per-worktree, held in the instance's own admin dir and moved by `git commit` in the instance (L11).
- Snapshot ref. `refs/mock-snapshot/<enc ns>@<enc wt>`: the ref-held recovery copy of one instance, with history carried by parents.
- User cache. `${XDG_CACHE_HOME:-$HOME/.cache}/mockspace/<repo id>/<ns>/<wt>/<stamp>/`. It holds plain copies kept for 24 hours, and the newest entry is always kept.
- `mock://<ns>/<p>`. The engine's address of a mock-tree path. It has two resolutions:
  - working, `<worktree root>/<ρ>/<ns>/<p>`, where the instance exists;
  - committed, the blob `<tip>:<p>`.
  The address is never handed to git as a URL, because git dispatches `mock://` to a remote helper (`013_probes/p4`, last block).
- Shadow tree, `@`. The directory `@/` at the top of a mock tree mirrors the git tree. φ(`/q`) = `@/q`. A shadow file's kind is its suffix, so `@/crates/foo/README.md.tmpl` is the template whose output is the git-tree file `/crates/foo/README.md`. φ is injective across all mock trees, and a second claim on one git path is refused.
- Pre-write copy. The snapshot taken by the `fsmonitor-watchman` hook before a git command reads the index (L15, L25).
- Repair. The engine's automatic, unforced re-publication of a mirror ref the remote has lost (D7).

## 1. Ledger

Holder codes:
- 001 means seat 001's files 001, 003, 005, 008 and 012;
- 002 means seat 002's files 002, 004, 006, 009 and 013;
- 007 means mine: 007, 010 and 014.

Clone shapes:
- plain;
- opted-in, the main worktree;
- linked, a linked worktree of an opted-in clone;
- bare+linked;
- mirror, a `clone --mirror`.

### 1.1 Agreed, carried from 010 with their status after this round

L1 to L9, L12 to L14, L17 and L18 of `010_consolidation.md` stand unchanged. They are: the native worktree and git's merge; old commits do not reach a disjoint path; same depth for data with `..` paths; never unhidden; pollution and what the refused name does about it; the tracked ignore line; `update` is fetch plus `merge --no-overwrite-ignore`; locks; double-f `clean`; mirror push damage; the namespace prefix rule; the copied index; pairing; no `core.bare` edit. Their probes and holders are as listed there. Six change.

L3 changes. The depth rule is now stated against the instance root. ρ is exactly one component, so `<ρ>/<ns>` sits at depth two, where the manifests were authored.
- Held by 001, 002 and 007.
- Probes: `012_probes/u1` part c (`.mock` and `git~1` both build, `cargo metadata`), `013_probes/p4` (`git~1`, `.mockroot`, `GIT~1` and `.git ` resolve; `deep/root` breaks), and `010_probes/c3` (37 relative dependencies: 15 to outside the mock directory, 22 inside, none into a sibling).

L10 changes. Op ruled that the remote holds `refs/mock/<ns>` on the same repository by default and that a second repository is an option only (`000_op.md`, Q3 answer). Probes are as in 010. A real host is still untested.

L11 changes. The working ref is `refs/worktree/mock/<ns>`, which survives a mirror clone's pruning fetch and a gc and is not published by `push --mirror` from the main dir.
- Held by 007.
- Probes: `010_probes/c6` and `c2` (vanilla verbs on a symref HEAD; `update-ref --create-reflog` gives the working ref a reflog under default config).
- Seats 001 and 002 used `refs/mock-wt/...` and have not read c6, so a second read of this one is owed.

L15 changes. The pre-write copy ships on, through git-shook (op's words, below), subject to D8.

L16 changes. Per op, a second repository is optional, and the default is custom refs on the same repository.

L19 to L28 are new in this round.

L19. git refuses the exact path components `git~1` (any letter case), `.git ` and `.git.`. It is neither the tilde nor the word: `git~2`, `.git~1`, `~mock`, `~git~1`, `git~1x`, `@` and `.gitmodules` are all tracked.
- Held by 001, 002 and 007.
- Probes: `011_probes/n1`, `009_probes/p2`.
- These names are refused because on NTFS they alias `.git`: `git~1` is the 8.3 short name NTFS generates for `.git`, and Win32 strips the trailing space or dot. The refusal has been on by default on every platform since git 2.24.1 (CVE-2019-1352/1353; the exploit for 1353 is a directory named `git~1` that git under WSL wrote into `.git`). Sources: [protect_ntfs: turn on NTFS protection by default](https://repo.or.cz/alt-git.git/commit/9102f958ee5254b10c0be72672aa3305bf4f4704), [oss-security announcement](https://openwall.com/lists/oss-security/2019/12/13/1). The aliasing itself was not probed by anyone (no Windows). 001 inferred it at `012:15`, and these sources confirm the mechanism.

L20. A polluting commit under a `git~1` root cannot be made, and a crafted one cannot be checked out ("invalid path"), for every namespace under the root. The worst a crafted commit can do is make its own branch impossible to check out. It cannot cause a loss.
- Held by 001, 002 and 007.
- Probes: `010_probes/c4`, `013_probes/p4`, `008_probes/t2` row N.

L21. A `pre-push` hook is handed rewinds, explicit deletes and forced pushes of `refs/mock/*`, and can refuse them. It is not handed the deletions that `push --mirror` computes. It is absent in a plain clone and skipped by `--no-verify`.
- Held by 001, 002 and 007.
- Probes: `012_probes/m1`, `013_probes/p1`.

L22. An unforced, explicit fetch refspec `refs/mock/<ns>:refs/mock/<ns>` keeps the local mirror at the newest tip through any remote deletion or rewind, and fails loudly on both.
- Held by 001 (withdrawing the forced glob at `012:60`), 002 (withdrawing the tracking mapping at `013:61`), and 007.
- Probes: `010_probes/c1`, `012_probes/m1` case 4b.

L23. A repair push without force never lowers the remote. Across eight remote states, including a concurrent advance, a divergence and a recreation between check and push, the remote always ends at a tip at least as high as every tip anyone pushed.
- Held by 007, with 001 and 002 holding the same rule in their own forms.
- Probes: `014_probes/g4`, `013_probes/p2` (firing rule and idempotence), `012_probes/m1` case 4.

L24. After a deletion, the remote still holds the tip's objects until the remote's gc. Over `file://`, a clone that never had the data can fetch them by id. The ids are on record in every landing's `Mock-Ref` trailer.
- Held by 001 and 002, with 007 agreeing.
- Probes: `012_probes/m1` case 5, `013_probes/p3`.

L25. Cost of the pre-write copy per `git status`. These are spikes, not benches.

| Files | Arm | Time |
| --- | --- | --- |
| 3000 | no hook | 7 ms |
| 3000 | protocol floor, hook direct | 10 ms |
| 3000 | protocol floor, behind git-shook | 21 ms |
| 3000 | snapshot every command, behind git-shook | 71 ms |
| 30000 | no hook | 35 ms |
| 30000 | floor, hook direct | 49 ms |
| 30000 | snapshot every command, hook direct | 109 to 160 ms |

  No daemon runs, and nothing is left running between commands.
- Held by 001 and 007.
- Probes: `012_probes/f1` (3000 files), `014_probes/g2` (30000 files).

L26. The git-shook dispatcher replays stdin whenever stdin is not a terminal (`[ -t 0 ] || cat`, `/home/user/git-shook/git-shook`). Under `core.fsmonitor` this fails in two ways, wherever git runs with a non-terminal stdin:
- With an open stdin, every git command that reads the index hangs: a script, an editor subprocess, an agent harness. See `012_probes/f1_hang_evidence.txt` and `014_probes/g1_hang_evidence.txt`, where `git ls-files` waits on `cat` on the harness socket.
- In a read loop, the hook swallows the loop's input. `git ls-files | while read f; do git diff -- "$f"; done` visited 1 of 6 files with the hook and 6 of 6 without it (`014_probes/g1b`).

  A terminal is safe. The second failure is worse than the ticket states: the git command does not just hang, it silently changes what the person's script does.
- Held by 001 (the hang) and 007 (both).

L27. A tracked directory named `@` collides with git's alias for HEAD:
- `git log @` and `git diff @` without `--` die as ambiguous;
- `git checkout @` detaches HEAD and leaves the edit alone.

  The names `@@`, `@git`, `@tree`, `_git`, `=git` and `git-tree` behave as ordinary directories, where `git checkout <dir>` restores the directory from the index and discards the edit, as for any path.
- Held by 001 (`012_probes/u1` part a) and 007 (`014_probes/g3`).

L28. `mock://` is claimed by git's remote-helper dispatch.
- Held by 002.
- Probe: `013_probes/p4`.
- 007 agrees: the address stays inside the engine.

### 1.2 Disagreements, settled or located

D5. The instance root's default. 001 (`012:23-27`): an ordinary configured name by default, with `git~1` as documented hardening for Unix. Its reason is that `git~1` beside `.git` on NTFS is the dangerous kind of failure, writing into `.git`. 002 (`013:108-110`): `git~1` as the root closes the whole pollution class for every namespace, with the Windows question noted as an impression.

The evidence settles both halves:
- L19's sources confirm 001's inference. On an NTFS volume that generates 8.3 names, `git~1` at the worktree root is `.git` itself, and CVE-2019-1353 is exactly that. An engine creating `git~1/<ns>` there would write instances into the repository's admin directory. That is not a portability nuisance. It is a corruption of `.git`.
- L20 confirms 002: everywhere the name does not alias, it is the only mechanism in the panel that git enforces in every clone.
- 001's other reason, that "shared docs cannot spell it", does not survive op's addressing ruling. Everything that names a mock-tree path names it as `mock://<ns>/...`. The `..` paths are fixed by depth and not by name (L3). The root is ignored and untracked, so each clone may have its own without any commit noticing.

Settlement, composed per filesystem rather than picked:
- ρ is a per-clone value with a declared default list, `git~1` first and an ordinary name second (op names the ordinary one, Q1).
- At `instantiate` and at every `verify`, the engine takes `git~1` only after proving it is a distinct file from `.git`. Concretely: if `<root>/git~1` exists, it must not be the same file (device and inode, or Windows file id) as `<root>/.git`. If it does not exist, create it, then check the same thing, and remove it again on a match. The engine also requires `core.protectNTFS` to be unset or true.
- Otherwise it falls back to the ordinary name with the detection-only guard of L6.
- The tracked `.gitignore` line lists both names. `info/exclude` lists the one in use.

The proof is by identity, not by platform name, because WSL on a Windows volume is a POSIX system with NTFS aliasing, which is precisely the CVE. Status: settled on the evidence. The identity check itself has not been run on NTFS (P2).

D6. The physical shadow name. 001 (`012:43`): `@` is logical only, and the physical name is configured with a default that is not a revision alias. 002 (`013:29`): `@` is tracked normally, and tools put a pathspec after `--`.

The evidence: g3 shows `@`'s costs are ergonomic, not losses. Two everyday commands die loudly without `--`, and `git checkout @` detaches HEAD, which keeps the edit. Every other candidate behaves as a plain directory, where `checkout <dir>` discards the edit as for any path. Op asked for "a @ dir if allowed", and it is allowed. The detached HEAD is already a named residual state that `verify` reports and that engine verbs refuse until HEAD is back on the working ref (R5).

There is one structural point neither seat made. The physical name is committed inside every mock tree's history, so it is not a per-clone setting. A "configured" name would be a constant of the repository, and changing it later is a rename across every mock tree.

Settlement: physical `@`, as op wrote it. The `--` rule goes in the guide, and R5 covers the detach. A configuration knob is not offered, because it would fork the shadow tree's spelling between repositories for an ergonomic gain. Status: settled on g3 and u1.

D7. The repair trigger. 002 (`013:77-93`): fire when the remote is absent or a proper ancestor of a monotone seen tip, and push with `--force-with-lease`. 001 (`012:58`): on every contact, compare `ls-remote` with the mirror and push without force.

The evidence:
- With the fetch unforced (L22), the mirror ref is itself monotone. 002's separate seen ref, which existed because 002 still had a forced mirror at `013:25`, is the mirror ref.
- When the rule fires, the push is always a fast-forward or a creation, so it never needs force. g4 shows that an unforced push of the seen tip ends with the remote at the maximum in every state 002's lease was protecting against: a concurrent advance (rejected, c4 kept), a divergence (rejected, x2 kept), a recreation (rejected) and a second rewind (succeeds, c3).
- A lease is force with a guard. It admits a non-fast-forward overwrite whenever the guard matches, so if the engine ever computes the seen tip wrongly, the lease turns that bug into a remote rewind. An unforced push turns it into a refusal.

Settlement: 002's firing rule (it fires only for an absent or proper-ancestor remote), executed with 001's unforced push. Repairs are counted per window, with a stop and a warning past a threshold (002's bound, policy unmeasured). Status: settled.

D8. The fsmonitor numbers against the git-shook stdin defect. The numbers (L25) put the pre-write copy at 30000 files under a fifth of op's one-second bound, with no constant cost. That meets his bar ("sub second wait only on calling any git command, that's fine"). The defect (L26) makes the copy unshippable behind git-shook as it is, in a way op's bar does not measure: not slowness but a hang, and a silent change to the person's scripts.

Settlement: on, as op ruled, with three gates that are fixes rather than benches:
- git-shook replays stdin only for events whose hook git feeds on stdin: `pre-push`, `pre-receive`, `post-receive`, `post-rewrite`, `reference-transaction`, `proc-receive` and `push-to-checkout`. That is the ticket `git_shook_s_dispatcher_reads_stdin_whole_so_an_fsmonitor_hoo`, widened by g1b to cover the input swallowed in a read loop.
- The mock entry for `fsmonitor-watchman` is side-effect only and prints nothing. git-shook passes git the answer of the person's own fsmonitor entry if there is one, or the protocol floor otherwise, so no person's optimisation is replaced. This is a git-shook design item and is unprobed.
- The entry snapshots only when the tree id differs from the last snapshot (the g2 recipe), so the steady-state cost is one `add -A` over a copied index.

The harness bench at 100000 files with real arms (P7) prices the borderline region. It is not a gate, because op already said "on". Status: settled. The fixes are owed.

D2, from 010 (common-dir instances for namespaces no tool reads by path). It stays located: no census and no concurrency probe have run. op's "no literal mock/ dir" changes nothing here.

D3, from 010 (fsmonitor shipped off). Superseded by op's "I would like it to be on" and by D8.

D4, from 010 (working-ref name). Superseded by L11 (`refs/worktree/mock/<ns>`), and a second read is owed.

### 1.3 Op's rulings, and where each lands

| Ruling | Where it lands |
| --- | --- |
| No literal `mock/`; `mock://<ns>` or `<ρ>/<ns>`; root configured | Glossary, D5, 2.2 |
| Templates and their kind go into a mock tree under `@`, mapping to git-tree paths | Glossary, D6, 2.3 |
| The terms are formalised | Section 0 |
| Same-repository custom refs by default; second repository optional | L10, L16, 2.1 |
| The mirror guard: a hook where possible, else a churn-free automatic re-push; what holds the tip when no clone does | L21 to L24, D7, 2.6, R4 |
| fsmonitor on, through git-shook, sub-second, no constant cost | L25, L26, D8, 2.5 |
| The registry stores leave `refs/heads/` | Glossary (kind), 2.1, 2.8 |

## 2. The build

### 2.1 Refs

- Published: `refs/mock/<ns>` on the code repository's remote. The prefix is a declared setting that defaults to `refs/mock/`. A dedicated remote may name another URL, which is op's optional second repository, and changes no other part of the layout (L16).
- Per opted-in clone and per namespace, a remote `mock-<ns>`:
  - `url` = the code remote's URL unless declared;
  - `fetch = refs/mock/<ns>:refs/mock/<ns>`, explicit and unforced (L22);
  - `push = HEAD:refs/mock/<ns>`;
  - `tagOpt = --no-tags`.
  A plain `git fetch --all` in an opted-in clone therefore brings that clone's mock trees and nothing else.
- Mirror ref: `refs/mock/<ns>`, monotone.
- Working ref per instance: `refs/worktree/mock/<ns>`, made with `update-ref --create-reflog`, with HEAD a symref onto it.
- Snapshot ref per instance: `refs/mock-snapshot/<enc ns>@<enc wt>`, made with `--create-reflog`. `enc` percent-encodes `%`, `/` and `@`.
- Every kind uses the same layout: data, registry and history. The registry tree is no longer a branch on the remote (2.8).
- Kept and never rewritten: the shipped `refs/mock-backup/<ns>`.
- No mock ref is ever created under `refs/heads/`, `refs/tags/` or `refs/remotes/`, locally or on the remote.
- Namespaces are prefix-free and are refused at declaration (L13).

### 2.2 Instance root and instances

- ρ per D5:
  1. `git~1` if the identity check passes and `core.protectNTFS` is not false;
  2. else the declared ordinary name (Q1).
- ρ is a single component (L3). A multi-component root is refused at declaration with the reason.
- One instance per git-tree worktree per `data` namespace, at `<worktree root>/<ρ>/<ns>`. The working copy is per worktree; the history is per repository.
- Ignored twice: a tracked `.gitignore` line on the code branch for every name in the default list, and an `info/exclude` line for the one in use (L4, L6).
- Locks (L8):
  - every instance is locked, with reason `mockspace instance <ns>`;
  - its parent linked worktree is locked, with reason `holds mockspace instance <ns>`;
  - only `retire` unlocks, and only after a snapshot.
- `registry` and `history` kinds have no instance. Only the engine reads them, as committed content (`mock://<ns>` committed resolution). That is the only region where the "no tool reads it by path" premise of D2 is known to hold.

### 2.3 The shadow tree `@`

- φ(`/q`) = `@/q` inside a mock tree. One function, `shadow(git_path) -> mock://<ns>/@/<git_path>`, and its inverse are the only way the engine relates the two trees.
- The adjacency readers that 001 found become calls to it: `src/render_design.rs:503`, `:577`, `:582` and `:626`; `src/lint/mod.rs:112` and `:471`; `src/registry/mod.rs:615`; `src/document.rs:892` and `:913`; `src/render_agent/content.rs:409` (census at `012:41`).
- Discovery lists `<ρ>/<ns>/@/<crate dir>`, not the crate directory.
- φ must be injective across every mock tree: the declaration, `verify` and a lint refuse two shadow files for one git path.
- Which namespace carries the shadow tree is op's naming (Q2). The default is one namespace, and `@/` is permitted in any.
- Generation writes git-tree files from shadow files. Nothing in the git tree names a mock path physically, and links in generated output use `mock://`.
- Open (P5): shadow files that carry relative paths to git-tree neighbours resolve against the shadow tree.

### 2.4 Engine verbs as git commands

In each verb: `D` is the instance path, `W` = `refs/worktree/mock/<ns>`, `M` = `refs/mock/<ns>`, and `X` = `mock-<ns>`. "Snapshot" means 2.5.

- `instantiate <ns>`:
  1. refuse during a merge, cherry-pick or rebase in the code worktree;
  2. choose ρ per 2.2;
  3. configure `X`;
  4. `git fetch X`, where `couldn't find remote ref` on a namespace that was never published means unborn, and on a published one is remote damage (2.6);
  5. move any existing non-instance `D` into the user cache, never deleting it;
  6. `git worktree add --detach D M`;
  7. `git -C D update-ref --create-reflog W M`;
  8. `git -C D symbolic-ref HEAD W`;
  9. add the ignore lines;
  10. lock;
  11. snapshot.
- `status`: `git -C D status --porcelain=v2 --branch`, plus `rev-list --left-right --count W...M`, plus the repair state of 2.6.
- `update`:
  1. snapshot;
  2. contact (2.6);
  3. `git -C D diff --name-only W M` gives the incoming paths, and ignored files at those paths go into the snapshot and the cache;
  4. `git -C D merge --no-edit --no-overwrite-ignore M`;
  5. a conflict is git's conflict.
- `commit`:
  1. snapshot;
  2. the repository's lints run as a `pre-commit` hook through git-shook, recognising an instance by its HEAD symref under `refs/worktree/mock/`;
  3. `git -C D add -A`, then `git -C D commit`.
  Filters, LFS pointers, hooks and signing are git's. This removes `--no-filters` at `mock/crates/mockspace-store/src/instance/files.rs:228`, `instance/backup.rs:320` and `ref_ops.rs:303`.
- `push`: contact, then `git -C D push X HEAD:M`, unforced. On a rejection: `update`, then one retry.
- `restore`: snapshot, then `git -C D restore --source=<snapshot> --worktree -- <paths>`.
- `retire`:
  1. refuse unless clean, with `W` an ancestor of `M`;
  2. snapshot;
  3. unlock;
  4. `git worktree remove D`.
- `verify`, all read-only:
  - locks are present;
  - HEAD is the symref;
  - ρ passes the identity check;
  - `core.protectNTFS` is not false when ρ is `git~1`;
  - no git-tree path under ρ is tracked (`git ls-files -- <ρ>`);
  - the ignore lines are present;
  - `X` is configured as in 2.1;
  - no mock ref sits under `refs/heads/`, locally or by `ls-remote`;
  - φ is injective;
  - namespaces are prefix-free;
  - the repair state.

  `verify --fix` runs `git worktree repair` and re-adds lines. It never deletes.
- `migrate`: sections 2.7 and 2.8.

### 2.5 Snapshot, user cache, pre-write copy

The snapshot, before every engine step that writes into `D`:
1. copy `D`'s index (`git -C D rev-parse --path-format=absolute --git-path index`) to a temporary file `I`;
2. `GIT_INDEX_FILE=I git -C D add -A`;
3. add the at-risk ignored files with `add -f` (the incoming paths for `update`, every ignored file under a size cap otherwise);
4. `write-tree`;
5. if the tree equals the current snapshot's tree, stop;
6. otherwise `commit-tree` with parents (snapshot tip, `W`), then `update-ref --create-reflog` on the snapshot ref.

The user cache is written in the same step: a plain copy of every file that differs from `W`, plus a `MANIFEST` naming the snapshot commit. It is pruned at engine start to 24 hours, always keeping the newest entry. It is the only layer that survives deleting the clone.

The pre-write copy is an `fsmonitor-watchman` entry through git-shook, on by default (op), gated by the D8 fixes:
- it runs the snapshot recipe for every instance of the worktree when the tree id changed, prints nothing, and leaves the answer to the person's entry or to git-shook's floor;
- it carries a re-entry guard in the environment;
- it covers the destroyers L15 lists;
- it does not cover `worktree remove -f -f`, which the locks gate, or `rm -rf`;
- it is bypassed by `-c core.fsmonitor=false`.

Op demands git-shook for mockspace use, so `instantiate` refuses to proceed without it, or installs it, which is op's choice between the two (Q3).

### 2.6 The mirror guard and the repair

Three layers, none of them load-bearing on its own.

- Layer 1, refusal by hook. A `pre-push` entry through git-shook refuses:
  - any delete of `refs/mock/*`;
  - any non-fast-forward of `refs/mock/*` (`merge-base --is-ancestor` of remote sha against local sha);
  - any push that carries a clone-local ref name (`refs/remotes/*`, `refs/mock-snapshot/*`), which only `--mirror` sends. This stops the first mirror push from a hooked clone (`012_probes/m1` case 1).

  Its limits are measured: blind to mirror-computed deletions, absent in plain clones, skipped by `--no-verify` (L21). This is op's "hook to --mirror calls", and the answer is: partly.
- Layer 2, repair on contact. Every engine command that already talks to the remote (`update`, `push`, `verify`, the landing gate) first runs `git ls-remote X M`, which gives `r`, the remote tip. With `s` the mirror ref:
  - if `r` is absent and `s` exists: `git push X s:M`, unforced;
  - if `r` is a proper ancestor of `s`: the same push, a fast-forward;
  - if `s` is an ancestor of `r` or equal to it: nothing (fetch as usual);
  - otherwise (incomparable): nothing automatic, and `status` reports a divergence for `update`'s merge to resolve.

  It is churn-free in op's sense:
  - it pushes only when the remote is deficient;
  - it is idempotent;
  - it never moves `refs/heads`, so no plain clone and no branch listing sees it;
  - it never fights a legitimate advance (`013_probes/p2` scenarios 1 to 8, `014_probes/g4`).

  Repairs are counted per 24 hours, and past a threshold the engine stops and warns (the threshold is policy).
- Layer 3, memory. Every opted-in clone holds the newest tip it saw in four places a remote event cannot touch: the mirror ref, the working ref, the snapshot ref and the user cache (L22). Every `clone --mirror` holds it too. Every landing's `Mock-Ref` line holds its id.

Op asked what holds the tip when no clone holds it. In order:
1. The remote's own unreachable objects, until the host's gc prunes them. The `Mock-Ref` id allows a fetch by id, which works over `file://` (L24); whether hosts allow it is unprobed (P1).
2. Any `clone --mirror` taken before the damage.
3. The optional second remote, if published to (Q4).
4. Past those it is lost.

Reaching that state takes a destructive push, plus the host's gc, plus the loss of every clone that ever fetched or pushed that tip together with their caches. It is two independent failures at least, and with a second remote three. It is not zero, and the guide says so.

A deliberate purge (for example of a leaked secret) is indistinguishable from a mirror accident, and layer 2 will undo it. That needs an explicit purge epoch that every clone honours. It is undesigned and is Q5.

### 2.7 Migration of data mock trees, with no loss

The rules are carried from 010 2.6:
- each step is idempotent, and a stopped run is finished by the same call;
- a snapshot goes into a ref and into the user cache before anything is removed or overwritten;
- an original is removed only after a byte comparison against its new home;
- no remote ref is deleted before its replacement is verified by `ls-remote`;
- `migrate --undo` restores from those copies and pushes nothing by itself.

Three amendments for this round:
- The target is `<ρ>/<ns>` with ρ chosen per 2.2, and every template moves to its shadow path, `@/<git path>.tmpl`. A template's kind is its suffix, so nothing is renamed but its directory.
- In the shipped v2 instances, templates were held by pattern beside sources (`src/migrate_refs.rs:261` collects them through `cfg.mock_dir.join(p)`). Each moves by `git -C D mv` within the instance, from its interleaved path to `@/<path>`. The `.md.tmpl` paths in the git tree are retired by one code-branch commit.
- v2 step V2.2 (remote `refs/heads/mock/<ns>` to `refs/mock/<ns>`) uses the sweep of 2.8, so clones still running the old engine cannot fork the history.

### 2.8 Migration of the registry stores

Today each store is declared by `RefDecl` (`mock/crates/mockspace-registry/src/stores.rs:286-310`): namespace `registry/<store>` or the test history's, local `refs/mock/<ns>`, remote `refs/heads/mock/<ns>` by default (`form = "branch"`, `stores.rs:308`; `TICKET_REF` at `test_world.rs:44`). Op ruled that they leave `refs/heads/` (`000_op.md:117-123`).

- R.0 Snapshot: `git update-ref refs/mock-snapshot/<enc ns>@migrate <local refs/mock/<ns>>` for every store, plus a bundle of each store into the user cache (`git bundle create <cache>/<ns>.bundle refs/mock/<ns>`). Registry rows are the one place where a clone may hold commits no other clone has, so the bundle is the off-repository copy.
- R.1 Fetch the current remote branch: `git fetch origin refs/heads/mock/<ns>` into `FETCH_HEAD`. Then merge it into the local store through the store's own merge hook (the store's `update`, unchanged), so the local ref descends from both.
- R.2 Publish: `git push origin <local>:refs/mock/<ns>`, unforced, then verify with `git ls-remote origin refs/mock/<ns>`.
- R.3 Reconfigure: the store gets its `mock-<ns>` remote with the 2.1 lines. `form` loses the `branch` value, and `RemoteForm::Branch` and `branch_form` (`mock/crates/mockspace-store/src/refs.rs:56-62`) are deleted, along with `ref_form`'s now-only caller shape. The forced `fetch_refspec` (`refs.rs:73-77`, `+<remote>:<local>`) becomes the explicit unforced form.
- R.4 Retire the branch: `git push origin --delete refs/heads/mock/<ns>`, only after R.2 is verified and only if its tip is an ancestor of the published ref.
- R.5 Sweep, for clones still on the old engine. Every new-engine contact checks `ls-remote origin refs/heads/mock/<ns>`. A branch reappears when an old engine pushes, because a push recreates it. When that happens:
  1. fetch it;
  2. merge it into the store through the store's hook;
  3. publish to `refs/mock/<ns>`;
  4. delete the branch once its tip is an ancestor of the published ref.

  No row an old engine writes is lost, and the branch lives only between an old engine's push and the next new-engine contact. Whether the shipped engine reads a missing remote branch as an empty store, and so rewrites it from scratch, is P4. If it does, R.4 waits until no old engine remains.
- R.6 Declaration: the store and `data` namespaces stay prefix-free. `src/data_refs.rs:67-68`, `:83-99` already refuse a data namespace that a store uses, and the prefix rule extends that to `registry` against `registry/<store>`.

### 2.9 Test matrix, owed as contracts and red first

All of 010 2.7, re-cut on these axes:

- Arms:
  - root `git~1`;
  - root ordinary;
  - root `git~1` with `core.protectNTFS=false` (`verify` must refuse);
  - a filesystem that aliases `git~1` (P2; on Linux simulated by a symlink `git~1 -> .git`, which `instantiate` must refuse by the identity check).
- Clone shapes: plain, opted-in, linked, bare+linked (refspec unset, tracking, mirror-style), and mirror.
- Every tree-writing git verb listed in 010 2.7, run in the git tree and in the instance, plus `git checkout @`, `git log @` and `git diff @` in an instance (L27).
- Remote rows:
  - the c1 table;
  - `push --mirror` from plain, current, stale and `--no-verify` clones, each followed by contact in a third clone;
  - the g4 table for repair;
  - the 013 p2 scenarios, including the repair count threshold;
  - deletion followed by the remote's gc followed by fetch by id.
- Registry rows:
  - R.0 to R.6, each interrupted and re-run;
  - an old-engine push after R.4, swept;
  - two clones with divergent rows during the migration.
- Hook rows:
  - the pre-write copy under a terminal, an open non-terminal stdin, a closed stdin, and a `while read` loop, which must visit every line (g1b);
  - a person's own fsmonitor entry chained;
  - `-c core.fsmonitor=false` as a documented bypass.
- Existing tests:
  - the LFS test inverted to demand the pointer through real git-lfs;
  - the reftable test `#[ignore]` with a reason;
  - the four-row old-commit test replaced by the matrix.

## 3. Residual loss paths

- R1. In the git tree:
  - `clean -ffdX` or `-ffdx` (ignored root);
  - `rm -rf`;
  - `worktree remove -f -f` of a locked parent.

  These lose the uncommitted instance edits made since the last copy. The bound is the newest of three copies: the engine snapshot, the pre-write copy (all of these except `rm -rf` and `worktree remove -f -f` run a git command that reads an index first, L15), and the user cache.
- R2. Ordinary root only (D5 fallback). A commit tracking a regular file under ρ overwrites an unrelated edit when someone moves onto it, silently even by plain `checkout` or `merge`. The bounds are detection (`verify`, `pre-push`, a CI lint) and R1's copies; the pre-write copy runs before such a move (`012_probes/f2`). Under `git~1` this row does not exist (L20).
- R3. In the instance: `reset --hard`, `checkout -f`, `checkout <dir>`, `clean -ff*`. These are git's own destroyers. The bound is R1's copies, plus the working ref's reflog and HEAD's (`010_probes/c2`).
- R4. On the remote: a mirror push, a forced push or a delete from a clone without the hook, or with `--no-verify`. The bounds are layers 2 and 3 of 2.6. It becomes a real loss only on the double failure stated there.
- R5. HEAD moved off the working ref (`checkout -b`, `switch --detach`, `checkout @`). Nothing is lost. `verify` names the state, and engine verbs refuse until HEAD is back on the working ref.
- R6. Mirror clones: the snapshot ref is pruned by `fetch --prune` (`010_probes/c5`), while the working ref survives (`c6`). The bound is the user cache and the working ref.
- R7. Deleting the clone loses whatever was neither pushed nor in the 24-hour cache.
- R8. A registry row that exists only in one clone, if that clone is lost during R.0 to R.5. The bound is the bundle in the user cache (R.0).

## 4. Questions that are still op's

Q1. The ordinary fallback name for ρ, used where `git~1` fails the identity check. Options:
- (a) a dot name, `.mock`: hidden in listings, so it is invisible to a person browsing, which suits data people do not browse;
- (b) a visible name, `mockdata`: browsable, and it sits beside source directories at the root;
- (c) `~mock`: a legal name with no protection (L19), and an unquoted `~mock` expands in shells wherever a user `mock` exists (impression).

Q2. Which mock tree carries the shadow tree `@`, and its namespace name. Options:
- (a) one namespace, named by op ("something other than templates"), holding every shadow file;
- (b) a shadow tree in every namespace that wants one, with φ checked across all of them.

The cost of (b) is a cross-namespace injectivity check on every declaration and every commit.

Q3. git-shook: required or auto-installed (op offered both). Options:
- (a) `instantiate` refuses without it, which is explicit;
- (b) `instantiate` installs it, which is frictionless and writes the person's hooks path and config on their behalf.

Either way, the two git-shook fixes of D8 come first.

Q4. Publishing to a second remote by default, for the double-failure case of 2.6. Options:
- (a) off: op's default, same-repository refs only;
- (b) on where declared, giving a third failure domain with no server configuration.

The cost of (b) is a second repository's access control, which must be kept in step.

Q5. A deliberate purge of a mock tree's history (a leaked secret, a gigabyte asset). Layer 2 will re-publish what a purge removes. Options:
- (a) a purge epoch, a ref every clone honours, which stops repair and rewrites local mirrors;
- (b) purge by moving to a new namespace and retiring the old one;
- (c) no purge.

The cost of (a) is a new mechanism. The cost of (b) is churn in every `Mock-Ref` id.

## 5. Probes owed, ranked by what each would change

- P1. A real host, for each host op uses:
  - push, fetch and listing of `refs/mock/*`;
  - `push --mirror` damage;
  - fetch by id after a deletion;
  - how long unreachable objects survive.

  Decides whether same-repository refs work at all there, and the recovery window of 2.6. It needs a throwaway repository that op authorises.
- P2. The D5 identity check on NTFS with 8.3 names on and off, under Windows git and under WSL. Decides whether `git~1` is ever taken on Windows, and proves the check catches the alias.
- P3. The git-shook fixes of D8 written, then g1b and f1 re-run. This is the gate on L15 shipping.
- P4. The shipped engine's behaviour when its remote branch disappears or reappears. Decides R.4 timing and the v2 sweep.
- P5. A census of relative paths in templates that point at git-tree neighbours. Decides whether φ needs path rewriting.
- P6. Follow-ups on the per-worktree working ref: rebase and merge rows, gc reachability once HEAD has moved off it, and `worktree repair` after a move. Decides L11 for good.
- P7. The pre-write copy in the harness at 30000 and 100000 files, with arms none, floor, snapshot-if-changed, snapshot-every and the person's chained fsmonitor. Prices the region near op's one-second line.
- P8. `adopt` of a pre-migration feature branch through `git apply -3`. Decides V1.7 of 010.
- P9. Two writers on a shared common-dir instance with the landing gate. Decides D2.
- P10. Reftable on git 2.46 or later across the matrix.
- P11. LFS through a real server with the per-namespace remote.
