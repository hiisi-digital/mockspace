# Data ref architecture: consolidation (seat 003, Fabian Giesen)

Status: canon candidate for the design rounds, unratified. Nothing here governs until op ratifies it. Where it disagrees with a seat, the seat's file and its probes are cited so op can judge the evidence and not my summary of it.

Gates. Canon gate: no `canon_paths` is declared. What governs is `000_op.md:69` (no data loss; machinery that makes loss impossible for someone who does not know it, the way git does; a recovery cache under every overwriting step without leaning on it; git tooling, merge drivers and LFS unchanged) and op's ruling during the audit (`000_op.md`, section "During the audit": data refs are not branches a forge lists, a plain clone does not fetch them, only a clone that asks gets them). Every build item below was checked against those two passages. Two items are not decided by them, and both are listed as op's questions in section 4: the `git~1` name and the template relocation. 008 and 009 each hold a reading of canon fit. This file is a third, and no claim here about what the canon permits rests on one seat alone.

Test gate. My whole-suite run from the audit is still going: the stop tool named in the coordinator's message is not one I have, and I was told not to kill. As of writing: 2071 passed, 1 failed, 54 ignored. The failure is the known red `every_migration_keeps_a_bare_clone_with_linked_worktrees`. The git-shook test passed with the tool on PATH. Partial output: `010_probes/suite_partial_output.txt`. The two suite defects stand as found in 001 and confirmed in 007: the reftable test returns green without running (`tests/refs_backup.rs:325-327`), and the LFS test pins the bytes op refused (`tests/refs_consumer_migrations.rs:171-183`). The verb matrix of the shipped suite is four rows (`tests/refs_daily_history.rs:106-111`).

Evidence. Every probe cited is committed beside the file that cites it. They are ad-hoc spikes on git 2.43.0 over `file://` remotes or local repositories, and they show behaviour. No timing in any of them is a bench. No seat has run any of this against a real forge, git 2.46 or later, reftable, Windows, NTFS or a case-insensitive filesystem. Every "holds in" below is bounded by that.

## 1. Agreement ledger

Notation: 001 is seat 001 (files 001, 003, 005, 008), 002 is seat 002 (002, 004, 006, 009), 007 is me (007, 010). "Plain" is a clone that never opted in, "opted-in" is a normal clone with the data, "linked" is a linked worktree of an opted-in clone, "bare+linked" is a bare clone with linked worktrees, "mirror" is a `clone --mirror`.

### 1.1 Agreed

L1. The data is a native linked worktree of the code repository's own object store, with history merged by git's own merge. Merge drivers from the shared config and from the data branch's `.gitattributes` run with no engine code.
Held by 001, 002, 007. Probes: `001_probes/p5`, `002_probes/p5`, `005_probes/r3`, `007_probes/a12`, `008_probes/t7` item 5. Holds in opted-in, linked and bare+linked on git 2.43.0.

L2. A data path that no commit has ever tracked is untouched by every move to and from a pre-migration commit.
Held by 001, 002, 007. Probes: `001_probes/p2` control row, `002_probes/p4`, `007_probes/a1` (45 verbs, arms U, H, T, all keep on every old-commit row), `008_probes/t1`. Holds in the main worktree of an opted-in clone. In a linked worktree only the remove and move rows were run (`007_probes/a3`).

L3. Data whose files carry relative paths back into the tree must be physical, inside the tree, at the same depth as the path it replaces.
Held by 001, 002 (conceded at `006:19`), 007. Probes: `003_probes/q2`, `005_probes/r1`, `006_probes/p1` (cwd-relative invocations break, `--manifest-path` ones work), and `010_probes/c3`. The c3 census of `mock/research` found 37 relative path dependencies: 15 land outside `mock/`, 22 stay inside `mock/research`, none lands in a sibling under `mock/`. 13 of the 24 manifests climb out (recounted in 007).

L4. The data path is never unhidden. The `git add -A` gitlink an unhidden nested worktree produces breaks `git submodule update`, `status` and `foreach` with exit 128 in every clone, and stops a consumer's real submodules from populating.
Held by 001, 002, 007. Probes: `007_probes/a8`, `008_probes/t3`, `009_probes/p1` section 4.

L5. A commit from any clone that tracks a regular file at the data path destroys data when another clone moves onto it. Unhidden: by `checkout -f` and `reset --hard`. Hidden: by plain `checkout` and plain `merge` too. No hook in any clone can prevent it. An untrackable path component (`git~1`, also `.git` and its NTFS aliases) makes git refuse the commit and refuse to write a crafted one, in every clone, with no hook.
Held by 001, 002, 007. Probes: `007_probes/a1`, `a2`, `a9`, `008_probes/t2` (row N), `t9`, `009_probes/p1` section 3, `p2` section a, and `010_probes/c4`. Holds on default config. `core.protectNTFS=false` removes the refusal (`009_probes/p2` a2).

L6. A tracked `.gitignore` line on the code branch stops the accidental `git add -A` from a fresh clone that never ran the engine. It does not stop `git add -f`, and it is absent at every pre-migration commit, so `info/exclude` is needed as well.
Held by 001, 002, 007. Probes: `009_probes/p2` section b, `008_probes/t9`.

L7. `update` is `fetch` plus `merge --no-overwrite-ignore`, never `pull`. `pull` silently overwrites ignored files with incoming tracked ones, and has no such option. The snapshot must hold ignored files, which `ls-files -o -i --exclude-standard` plus `add -f` does.
Held by 001, 002, 007. Probes: `007_probes/a11`, `008_probes/t4`, `009_probes/p1` section 5.

L8. `git worktree lock` on the data worktree and on its parent linked worktree makes `worktree remove`, `remove --force` and `move` refuse. Only `-f -f` goes through. The lock on the data worktree keeps `worktree prune` off its admin dir after a move. Locks do not stop `clean`. After `mv` of a whole clone, `git worktree repair <paths>` restores it.
Held by 001, 002, 007. Probes: `007_probes/a3`, `008_probes/t5`, `009_probes/p1` section 2.

L9. Double-f `clean` reaches in-tree data. Unhidden: `-ffd` and `-ffdx`. Hidden: `-ffdX` and `-ffdx`. Single-f forms do not.
Held by 001, 002, 007. Probes: `007_probes/a1`, `008_probes/t1`, `009_probes/p1` section 1.

L10. On the remote the data lives at `refs/mock/<ns>`. Plain `clone`, `clone --bare`, and `fetch --all` or `fetch origin` in a plain clone get no data ref and none of its objects. `clone --mirror` gets it.
Held by 001, 002, 007. Probes: `007_probes/a16`, `008_probes/t7` item 1, `009_probes/p3`. Holds over `file://` only. No forge was tested.

L11. No data ref is ever a local branch. Each data worktree's HEAD is a symref onto a working ref outside `refs/heads/`. `commit` moves it, as do `merge`, `rebase`, `pull --rebase` with explicit arguments, `commit --amend`, `stash` and `cherry-pick`, all keeping the symref. `push --all` leaks nothing. Bare `git pull` and `git push` are inert ("not currently on a branch"). `checkout -b` moves HEAD off onto a branch, and `switch --detach` detaches it. A mirror-style fetch in a bare clone then works.
Held by 001, 002, 007. Probes: `008_probes/t7` items 3, 4, 7, 8, `009_probes/p3`, `010_probes/c2`, `007_probes/a13` (the failure it removes).

L12. `push --mirror` deletes or rewinds `refs/mock/<ns>` on the remote, and the client cannot prevent it. `receive.denyDeletes` does not cover refs outside `refs/heads/`. A server-side `update` hook does.
Held by 001, 002, 007. Probes: `007_probes/a16`, `008_probes/t8`, `009_probes/p3`.

L13. A namespace that is a path prefix of another is a D/F collision for refs, and the shipped declaration admits it (`src/data_refs.rs:95-96`, `mock/crates/mockspace-store/src/refs.rs:80-`). Declaration refuses it. `refs/mock-backup/<ns>` is kept untouched and the new refs use other prefixes.
Held by 001, 002, 007. Probes: `004_probes/p2`, `005_probes/r2`.

L14. The snapshot is built through a temporary index that starts as a copy of the data worktree's own index. That index-copy shape is about an order of magnitude cheaper than a fresh index. The figure is unpriced.
Held by 001, 002, 007. Probes: `003_probes/q3`, `006_probes/p2` (one racy case checked).

L15. A `core.fsmonitor` hook runs before the destructive porcelain verbs: `checkout -f`, `reset --hard`, `clean -ff*`, `stash`, `merge`, `rebase`, `restore`, `switch`, `pull`, `read-tree -u`, `checkout-index`. It also runs on `status` and `diff`. It does not run for `worktree remove`, `worktree prune`, `bisect start` or `rm -rf`, and `-c core.fsmonitor=false` bypasses it.
Held by 001, 002, 007. Probes: `007_probes/a14`, re-run by 001 in 008, and `009_probes/p4`. Whether it belongs in the build is disagreement D3.

L16. The dedicated data remote may point at a second repository URL with no loss of shared config: the worktree is still in the code repository's store.
Held by 002 and 007. 001 calls it an impression (`008:96`), so it is agreed by two. Probe: `009_probes/p5`.

L17. Pairing by the `Mock-Ref: <ns> <sha>` trailer stays. The gate requires a clean data worktree whose commit is an ancestor of the local mirror after a fetch.
Held by 001, 002, 007. Probe: `007_probes/a12` (ancestor check).

L18. No `core.bare` edit and no `extensions.worktreeConfig`. The red bare-clone test goes green by absence.
Held by 001, 002, 007. Probes: `007_probes/a12`, `008_probes/t7` item 8.

### 1.2 Located disagreements

D1. Local mapping of the remote data ref: identity (001: `+refs/mock/*:refs/mock/*`, `008:75`) against remote-tracking (002: `+refs/mock/<ns>:refs/remotes/mock-<ns>/<ns>`, `009:35`, `:43`).
Evidence for identity: an up-to-date clone's `push --mirror` keeps the remote ref (`009_probes/p3`), and the name reuses the shipped local mirror (`refs.rs:50-62`). Evidence for remote-tracking: a deletion shows on the next fetch and the tracking ref keeps the tip, where identity can rewind silently (`009_probes/p3` last line).
Settled now by `010_probes/c1`. The axis that matters is neither of the two the seats argued. It is the glob and the `+`. Results:
- 001's `+refs/mock/*:refs/mock/*` loses the local copy to a remote deletion when `fetch.prune` is on (`gone`), and rewinds silently on a remote rewind (`REWOUND`, rc 0).
- 002's `+…:refs/remotes/…` fails loudly on deletion and still rewinds silently (`REWOUND`, rc 0).
- An explicit, unforced refspec (`refs/mock/<ns>:refs/mock/<ns>` or the tracking form, no `+`, no glob) fails loudly on both: `couldn't find remote ref`, rc 128, and `rejected (non-fast-forward)`, rc 1. The local copy stays at the newest tip in all four damage-by-prune rows.
Between the two unforced forms, the remaining difference is the opted-in clone's own `push --mirror`. Identity re-pushes the ref: harmless when current, a rewind when stale, which every other clone then refuses loudly. Tracking deletes the ref on the remote and publishes `refs/remotes/*` there (`009_probes/p3`). I take identity, unforced, explicit, one fetch line per namespace. The engine never needs a forced fetch, because the engine never force-pushes data (`008_probes/t7` item 6). A deliberate history rewrite of a data ref is a separate, confirmed engine verb. Status: settled in my reading. Both seats owe a read of c1.

D2. Placement rule. 001 (`008:108`): in-tree at `git~1` only for data that points back into the tree, every other namespace a single worktree under the common git dir. 002 (`009:53-56`): in-tree for every namespace at the same depth, `git~1` for the one that needs it, an ordinary hidden name plus detection for the rest. Both premises rest on "one usable `git~1` name per tree" (`008:68`, `009:18`).
`010_probes/c3` and `c4` remove that premise. The untrackable component can be the top component: `git~1/<ns>` sits at the depth `mock/<ns>` had, holds any number of namespaces as sibling data worktrees, and git refuses a crafted commit under each (`c4`: `invalid path 'git~1/research/N.md'` on `checkout -f` and `merge`, edit kept). The root-bound relative paths resolve unchanged (`c4`: `../../../../bench-harness/x` reads). c3 shows this repository's research has no path into a `mock/` sibling, which is the only kind that would break.
What remains of the disagreement is the common-dir arm for namespaces that do not point out. For it: no double-f clean, no `worktree remove -f -f` and no pollution exposure (`002_probes/p4`, `003_probes/q2` b rows). One shared copy. Against it: tools and people read data by path, so a resolver-only location is a change of contract for every reader, and the shared copy across linked worktrees is the concurrency question no one has run (`003:70`, `005:63`).
My reading, which composes rather than picks: `git~1/<ns>` in-tree for every namespace a person or tool reads by path. The common-dir worktree only for namespaces only the engine reads. In this repository I know of none, and that is an impression with no census behind it. The probes that would settle it: the reader census (Q5 in section 4) and the two-writer concurrency probe on a shared common-dir worktree.

D3. `core.fsmonitor`. 001 keeps it out until benched (`008:49-56`). 002 adopts it as an optional best-effort layer now (`009:17`, `:58`). Both agree the safety claim does not rest on it. The difference is what ships.
Evidence against it: it runs on every git command, it is a single config value that collides with a person's own fsmonitor, it cannot tell destructive verbs apart, and it is unpriced. Evidence for it: it is the only pre-write point git has, and it covers exactly the double-f cleans, which the locks do not.
My reading: specified in the design, shipped off, enabled by a declared setting only after the harness bench has priced it. The bench arms: no hook; copy per command; snapshot per command with the tree id deduplicated; snapshot only when the data index's stat data changed; chained to a builtin daemon. The bench is the settling probe. It needs the harness, and nothing cheap settles it now.

D4 (minor). Snapshot and working-ref names. 001: `refs/mock-snapshot/<enc ns>@<enc wt>` and `refs/mock-wt/<enc ns>@<enc wt>`. 002: `refs/mock-cache/<ns>/<wt>` and `refs/mock-wt/<ns>/<wt>`. With the prefix refusal (L13) both avoid collision. The leaf encoding does not depend on that refusal, so I take it for the snapshot.
For the working ref, a probe found a better home than either: `010_probes/c5` shows a mirror clone kept current by `fetch --prune` deletes both shared local-only refs (`refs/mock-wt/…`, `refs/mock-snapshot/…`) as "deleted". `c6` shows a per-worktree ref `refs/worktree/mock/<ns>` survives the pruning fetch and `gc --prune=now`, is invisible from the main dir, and is not published by `push --mirror` from the main dir. HEAD can be a symref onto it, and `commit` moves it. It is published by `push --mirror` run from inside the data worktree. Not run: the rebase and merge rows of c2 against a per-worktree ref, and whether gc keeps a per-worktree ref's commit once HEAD has moved off it. Status: my proposal, unconfirmed by any seat.

## 2. The build, where agreed

### 2.1 Ref layout

- Remote: `refs/mock/<ns>`, one per namespace (L10). The prefix is a declared setting, defaulting to `refs/mock/`, so a host constraint changes a value and not the design (`008:74`).
- Opted-in clone: one remote per namespace, `mock-<ns>`, URL defaulting to the code remote's URL (L16 allows another). Settings:
  - `fetch = refs/mock/<ns>:refs/mock/<ns>`: explicit, unforced (D1);
  - `push = HEAD:refs/mock/<ns>`, so a vanilla `git push mock-<ns>` inside the data worktree works (`009_probes/p3`);
  - `tagOpt = --no-tags`.
- Local mirror: `refs/mock/<ns>`, the shipped name, never checked out and never the target of a working tree.
- Working ref, per data worktree: `refs/worktree/mock/<ns>`, created with `update-ref --create-reflog` (a default-config ref outside the logged prefixes has no reflog otherwise, `010_probes/c2`). HEAD is a symref onto it (L11, D4). Fallback if a c6 follow-up fails: the shared leaf name `refs/mock-wt/<enc ns>@<enc wt>`, with mirror clones refused at instantiate (c5).
- Snapshot ref, per data worktree: `refs/mock-snapshot/<enc ns>@<enc wt>`, with `enc` percent-encoding `%`, `/` and `@`, history carried by parents, created with `--create-reflog`. The worktree id is the basename of the worktree's admin dir, or `main`.
- Kept untouched: the shipped `refs/mock-backup/<ns>` and every commit the shipped pending files name (migration step V2.1).
- Never created: any data ref under `refs/heads/`, `refs/tags/` or `refs/remotes/`.
- The declaration refuses a namespace that is a path prefix of another (L13).

### 2.2 Placement

- Every namespace read by path lives at `git~1/<ns>`, at the depth `mock/<ns>` had (D2, L3, L5). This is the default if op accepts the name (Q1). If op rejects it, reading B applies: a same-depth ordinary name, with the pollution rows of L5 accepted as residual and bounded only by detection (section 3, R2).
- Per code worktree: each code worktree that uses the data has its own data worktree for each namespace. The working copy is per worktree, the history is per repository (`007` section 3, conceded at `009:26`). Data per code branch is rejected by all three seats.
- Hidden twice (L4, L6): a tracked `/git~1/` line in the code branch's `.gitignore`, and the same line in the common `info/exclude`.
- `verify` refuses `core.protectNTFS=false` when the untrackable name is in use (`009:57`).

### 2.3 Locks

The data worktree is always locked, with reason `mockspace data worktree <ns>`. Its parent linked worktree is locked with reason `holds mockspace data worktree <ns>`. The main worktree cannot be removed, so it needs none. Only `retire` unlocks, and it snapshots first (L8).

### 2.4 Engine verbs as git commands

In each: `D` is the data worktree path, `R` = `refs/worktree/mock/<ns>`, `M` = `refs/mock/<ns>`, `X` = remote `mock-<ns>`. "Snapshot" is 2.5.

- `instantiate <ns>`:
  1. refuse while `MERGE_HEAD`, `CHERRY_PICK_HEAD`, `REBASE_HEAD` or `rebase-merge/` exists in the code worktree's git dir;
  2. `git config remote.X.*` as in 2.1;
  3. `git fetch X`; on `couldn't find remote ref`, the namespace is unborn and the first commit is made by `commit`;
  4. if `D` exists and is not this data worktree, move it aside into the user cache, never delete it;
  5. `git worktree add --detach D M`;
  6. `git -C D update-ref --create-reflog R M`;
  7. `git -C D symbolic-ref HEAD R`;
  8. append the `info/exclude` line if absent;
  9. `git worktree lock` on `D` and on the parent if it is linked;
  10. snapshot.
- `status`: `git -C D status --porcelain=v2 --branch`, plus `git -C D rev-list --left-right --count R...M`.
- `update`:
  1. snapshot;
  2. `git fetch X`, where a non-zero exit stops the verb and names the cause: a deletion (`couldn't find remote ref`) or a rewind (`non-fast-forward`) is reported as remote damage with the recovery command, never acted on silently (D1);
  3. `git -C D diff --name-only R M` gives the incoming paths, and the ignored files present at those paths go into the user cache and into the snapshot;
  4. `git -C D merge --no-edit --no-overwrite-ignore M`;
  5. a conflict is left as git's conflict.
- `commit`:
  1. snapshot;
  2. the repository's lints run as a `pre-commit` hook from the shared hooks directory, which recognises the data worktree by its HEAD symref under `refs/worktree/mock/` (`004_probes/p3` showed the hook runs and sees `GIT_DIR`);
  3. `git -C D add -A`, then `git -C D commit`, so filters, LFS, hooks and signing are git's (L1). This deletes the `--no-filters` hashing at `mock/crates/mockspace-store/src/instance/files.rs:228`, `instance/backup.rs:320` and `ref_ops.rs:303`.
- `push`: `git -C D push X HEAD:M`, unforced. A rejection runs `update`, then retries once. Success is followed by `git fetch X`.
- `restore <ns> [<snapshot>] [<paths>]`: snapshot, then `git -C D restore --source=<snapshot> --worktree -- <paths>`.
- `retire <ns>`:
  1. refuse unless `D` is clean and `R` is an ancestor of `M`;
  2. snapshot;
  3. `git worktree unlock`;
  4. `git worktree remove D`;
  5. the working ref goes with the admin dir.
- `verify`, each a read-only check:
  - `git worktree list --porcelain` shows `D` locked;
  - `git -C D symbolic-ref HEAD` is `R`;
  - `git ls-files -- git~1` in every code worktree is empty;
  - both ignore lines are present;
  - `X` is configured as in 2.1;
  - no data ref exists under `refs/heads/`;
  - `git ls-remote X M` exists, and its absence is named as remote damage;
  - namespaces are not nested;
  - `core.protectNTFS` is not false.
  - `verify --fix` runs `git worktree repair` after a moved clone and re-adds missing lines. It never deletes anything.
- `migrate`: section 2.6.

### 2.5 Snapshot and user cache

Snapshot of `D`, taken before every engine step that writes into `D`:
1. `I=$(mktemp)`;
2. `cp "$(git -C D rev-parse --path-format=absolute --git-path index)" "$I"`;
3. `GIT_INDEX_FILE=$I git -C D add -A`;
4. `git -C D ls-files -z -o -i --exclude-standard -- <at-risk paths>` piped to `GIT_INDEX_FILE=$I xargs -0 -r git -C D add -f --`, where the at-risk paths are the incoming paths for `update` and every ignored file below a size cap for other verbs;
5. `T=$(GIT_INDEX_FILE=$I git -C D write-tree)`;
6. if `T` equals the tree of the current snapshot tip, stop;
7. otherwise `C=$(git commit-tree T -p <snapshot tip> -p R -m "snapshot <verb> <time>")` and `git update-ref --create-reflog <snapshot ref> C`.

The ref holds it through any `gc` (op's "Ref-held").

User cache: `${XDG_CACHE_HOME:-$HOME/.cache}/mockspace/<repo id>/<ns>/<wt>/<UTC stamp>/`.
- It holds a plain copy of every file that differs from `R` (modified, untracked, and ignored at-risk), plus a `MANIFEST` naming the snapshot commit.
- It is written in the same step as the snapshot.
- Each engine start prunes entries older than 24 hours, and never the newest entry per `<ns>/<wt>`.
- This is the only layer that survives deleting the clone.

The fsmonitor layer (D3) is specified and shipped off.

### 2.6 Migration with no data loss

Common rules:
- each step is idempotent and re-entrant, and a stopped run is finished by the same call;
- every step that removes or overwrites anything is preceded by a snapshot of what it touches into a ref and the user cache;
- an original is removed only after a byte comparison against its new home;
- no remote ref is deleted before the new remote ref is verified equal by `ls-remote`.

From v1 (data tracked on the code branch at `mock/<ns>`):
- V1.1 Refuse while the code worktree is dirty under `mock/<ns>` or an operation is in progress.
- V1.2 `T=$(git rev-parse HEAD:mock/<ns>)`, then `C=$(git commit-tree T -m "data: <ns> from <HEAD sha>")`. No history rewrite (005 conceded `subtree split`). Then `git push <code remote> C:refs/mock/<ns>`, unforced. A remote ref that already exists is fetched and merged, never overwritten.
- V1.3 `instantiate` at `git~1/<ns>`.
- V1.4 Copy the person's uncommitted and untracked files from `mock/<ns>` into `D` over the fresh checkout, so they arrive as uncommitted edits, and compare bytes.
- V1.5 Commit on the code branch: `git rm -r --cached mock/<ns>`, plus the `.gitignore` line, plus a lint entry naming `mock/<ns>` as retired. The commit message carries `Mock-Ref: <ns> <C>`.
- V1.6 After user-cache copy and byte comparison, remove the untracked leftovers at `mock/<ns>`.
- V1.7 Old feature branches that edit `mock/<ns>` meet a modify/delete conflict on merge. That is git's own signal. `adopt <branch>` carries their edits as `git diff <merge-base>..<branch> -- mock/<ns> | git -C D apply -3 -p3`. It is unprobed, and owed as P6.

From the shipped v2 (local `refs/mock/<ns>`, remote `refs/heads/mock/<ns>` or `refs/mock/<ns>`, instance excluded at `mock/<ns>`, sparse and `config.worktree`, `reference-transaction` hook, `refs/mock-backup/<ns>`, pending files):
- V2.1 First, before anything reads the instance: for every pending commit named in a file beside an instance record (`instance/pending.rs:11-20`), `git update-ref refs/mock-snapshot/<enc ns>@migrated-pending-<n> <sha>`. Leave `refs/mock-backup/*` untouched. Snapshot the instance directory into the user cache.
- V2.2 Remote. If the remote holds `refs/heads/mock/<ns>`, push its tip to `refs/mock/<ns>` (unforced), verify with `ls-remote`, and only then delete `refs/heads/mock/<ns>` on the remote. Clones still on the old engine then fail loudly on fetch. What the shipped engine does with a missing remote branch is owed as P4, because if it reads absence as unborn it must be blocked first.
- V2.3 Local. Keep `refs/mock/<ns>`, and reset its fetch line to the D1 form.
- V2.4 `instantiate` at `git~1/<ns>` from the instance's base commit, not the tip, so the person's edits apply against what they were made on. Copy the instance's files that differ from base into `D`, compare bytes, then `update`.
- V2.5 Undo the shipped config, each change only if the engine's own record says the engine made it:
  - `git sparse-checkout disable`, refused unless HEAD descends from the migration commit, since otherwise it would write old tracked bytes over the instance;
  - move `core.bare` back from `config.worktree` to the shared config and unset `extensions.worktreeConfig` (`src/data_refs/sparse.rs:198-216`);
  - remove the engine's `reference-transaction` hook if it carries the engine's marker;
  - drop the instance's exclude lines.
- V2.6 After user-cache copy and byte comparison, remove the old instance files at `mock/<ns>`.

Rollback: every pre-step state is in a snapshot ref and the user cache. `migrate --undo` restores files from those and re-pushes nothing on its own. This is op's "user cache liberally there".

### 2.7 Test matrix, owed as contracts and red first

Arms:
- placement: `git~1/<ns>`, and reading B;
- hiding: tracked line plus `info/exclude`, as built.

Clone shapes, each run as a separate world:
- plain (never opted in);
- opted-in main worktree;
- opted-in linked worktree;
- bare plus two linked worktrees, with the refspec unset, remote-tracking style and mirror style (`+refs/heads/*:refs/heads/*`);
- mirror clone with worktrees, refused or shown safe per D4.

Tree-writing git verbs, every one run in the code worktree and, where it applies, in the data worktree, each after an uncommitted edit, an untracked file and an at-risk ignored file in `D`. The assertion is that the bytes are on disk or in the snapshot ref or the user cache, that the worktree is intact and that HEAD is the symref.
- `checkout` (branch, `-f`, `-- .`, `<commit> -- <path>`, `--orphan`, `-b`), `switch` (plain, `--discard-changes`, `-C`, `--detach`, `--orphan`), `restore` (`-W`, `-SW`, `--source`, `:/`).
- `reset` (`--hard`, `--keep`, `--merge`, `--mixed`, `--soft`).
- `stash` (`push`, `-u`, `-a`, pathspec, `pop`, `apply`, `branch`).
- `clean`: the six f, x and X combinations.
- `merge` (plain, `-X theirs`, `--abort`); `pull` (merge, rebase, explicit and bare); `rebase` (plain, `-i`, `--onto`, `--autostash`, `--abort`); `cherry-pick` (plus `--abort`); `revert`; `am` (plus `--abort`); `apply` (`--index`, worktree).
- `bisect` (`start`, `good`/`bad`, `run`, `reset`).
- `read-tree -u` (`--reset`, `-m`); `checkout-index -a -f`.
- `sparse-checkout` (`init`, `set`, `add`, `reapply`, `disable`).
- `rm` (`--cached`, `-r`); `mv`.
- `worktree` (`add`, `remove`, `remove --force`, `remove -f -f`, `move`, `move -f -f`, `prune`, `repair`, `lock`, `unlock`).
- `submodule` (`update --init`, `update --checkout --force`, `deinit -f`, `absorbgitdirs`).
- `gc` (`--prune=now`, `--aggressive`), `maintenance run`, `repack -ad`.
- `update-ref` and `branch -f` against the working ref.
- `fetch` (plain, `--all`, `--prune`, mirror refspec), `push` (`--all`, `--mirror`, `--tags`).
- `clone` (plain, `--bare`, `--mirror`, `--recurse-submodules`).
- `filter-repo`/`filter-branch` on the code branch.
- Moves to a pre-migration commit, to a commit tracking a regular file at the data path, to a gitlinked commit, and to a commit crafted with `mktree`.
- `mv` of the whole clone.
- `rm -rf D`, as the residual row that asserts the snapshot and the user cache.

Remote rows: the c1 table as a test, and `push --mirror` from plain, from a current opted-in clone and from a stale opted-in clone, each followed by `update` in a third clone, which must fail loudly and keep its tip.

Engine rows: every verb of 2.4 interrupted at each git call and finished by the same call, which the shipped suite already does for the old verbs. Every migration step of 2.6 interrupted and re-run, with byte equality at the end. The LFS test inverted to demand the pointer through real git-lfs. The reftable test marked `#[ignore]` with a reason until a git that can run it is present.

## 3. Residual loss paths, stated exactly

- R1. In the code worktree, `clean -ffdX` and `-ffdx` with the data hidden; `clean -ffd` and `-ffdx` if a person unhides it; `rm -rf`; `worktree remove -f -f` of a locked parent. These lose the uncommitted data edits made since the last snapshot. Committed data survives in `R` and on the remote.
  - Bound: the snapshot ref and the user cache from the last engine verb. With the fsmonitor layer on, the last git command that read an index also counts, except `worktree remove` and `rm -rf`, which it never sees (L15).
- R2. Reading B only. A commit from any clone tracking a regular file at the data path overwrites an unrelated edit when someone moves onto it, silently and by plain verbs when hidden (L5).
  - Bound: detection by `verify`, pre-push and a CI lint, plus R1's caches. Under `git~1` this row does not exist on default config, and becomes reading B's row again if a person sets `core.protectNTFS=false`, which `verify` refuses.
- R3. Inside the data worktree: `reset --hard`, `checkout -f`, `clean -ff*`, a bare `git pull` that names the remote explicitly. This is git's own destroyer list for any worktree.
  - Bound: R1's caches. The working ref's reflog and HEAD's reflog recover moved commits (`010_probes/c2`).
- R4. On the remote: `push --mirror` from any clone deletes or rewinds `refs/mock/<ns>` (L12).
  - Bound: every opted-in clone keeps the newest tip and refuses the damage loudly (D1, c1). Recovery is one unforced push from a current clone. A self-hosted remote can stop it with an `update` hook. On a forge it is unprotectable from the client.
- R5. A person moves HEAD off the symref with `checkout -b` or `switch --detach` inside `D` (c2). Nothing is lost: commits go to the branch or to HEAD. A `checkout -b` branch is then published by `push --all`, which is op's noise case.
  - Bound: `verify` names the state, and the engine verbs refuse until HEAD is back on `R`.
- R6. Mirror clones. A pruning fetch deletes shared local-only refs (c5). The working ref in `refs/worktree/` survives it (c6). The snapshot ref does not.
  - Bound: the user cache. The engine refuses to instantiate in a clone whose fetch refspec covers `refs/mock-snapshot/` with prune enabled, until op answers Q4.
- R7. Deleting the clone loses everything not pushed and not in the user cache, for the 24 hours the cache is kept.

None of these is zero, and no sentence in a design built from this file may say otherwise.

## 4. Op's questions

Q1. The `git~1` component.
- (a) Accept `git~1/<ns>` at the top level, holding every namespace. The path survives no commit from any clone (L5, c4). Costs: an unreadable directory name at the repository root, a stated dependency on `core.protectNTFS` staying on, Windows and NTFS behaviour unexamined (P2), and every reader of `mock/<ns>` paths moves.
- (b) `mock/git~1` for the one namespace that points out, plus ordinary hidden names for the rest. Same name cost, but only one namespace is protected, and the rest carry R2.
- (c) Ordinary same-depth names everywhere (reading B). Readable, and R2 stays open with detection only. That is the loss op's intent forbids, so it would be an accepted residue.

Q2. Template relocation.
- (a) Templates move into a data namespace mirrored by crate path (`git~1/templates/<crate dir>/README.md.tmpl`), and the renderer resolves through one function. The pattern machinery, held files and interleave all go. Cost: a resolver change across every reader of `*.md.tmpl` (P5 counts them).
- (b) Templates stay beside sources and the shipped pattern machinery stays for that one namespace. Cost: the whole sparse, exclude and hook stack survives for one namespace, along with every loss row 001 and 002 measured against it.
- (c) Templates stay on the code branch. Cost: contradicts op's 11 November words in ticket `tickets-mockspace.toml:1500`.

Q3. `push --mirror` and a host that refuses `refs/mock/*`.
- (a) Accept R4 as bounded by D1's loud failure and one re-push. Cost: a window during which the remote has no data.
- (b) Put the data on a second repository URL (L16). That repository's own `refs/heads/` is its business, no code clone touches it, and a mirror push of the code repository cannot reach it. Cost: a second repository per project on the host, and its access control to keep in step.
- (c) A server-side `update` hook refusing deletion of `refs/mock/*`, self-hosted only. Cost: nothing on a forge.
If a host refuses `refs/mock/*` (the proxy at `refs.rs:16-19` did), (b) is the vehicle there. That changes a URL and not the layout.

Q4. Mirror clones with worktrees (R6).
- (a) Refuse to instantiate.
- (b) Keep the snapshot ref per-worktree as well, which loses it when the data worktree's admin dir goes.
- (c) Accept the user cache as the only snapshot layer there.

Q5. The fsmonitor layer (D3).
- (a) Shipped off, enabled by a setting after the bench.
- (b) Shipped on.
- (c) Not shipped.
Cost of (b) before the bench: an unpriced tax on every git command and a collision with a person's own fsmonitor.

## 5. Probes still owed, ranked by what each would change

- P1. A real host's handling of a push, fetch, listing and `push --mirror` of `refs/mock/*`, on each host op uses. Decides Q3 and whether the primary layout is usable at all. It needs a throwaway repository op authorises.
- P2. `git~1` on Windows (NTFS with 8.3 names on and off), on macOS (case-insensitive APFS) and in git for Windows; cargo and editors under that path. Decides Q1(a).
- P3. Follow-ups on the per-worktree working ref (`c6`): rebase, merge and pull rows against it; gc reachability once HEAD has moved off it; `worktree repair` after a move. Decides D4 and R6.
- P4. The shipped engine's behaviour when its remote branch disappears. Decides whether V2.2 must block old engines first.
- P5. The reader census for `mock/research`, `research/`, `*.md.tmpl` and `design_rounds`. Decides Q2 and the size of the path move.
- P6. `adopt` of a pre-migration feature branch through `git apply -3`, with renames into and out of the path. Decides V1.7.
- P7. The fsmonitor bench in the harness with the arms in D3. Decides Q5.
- P8. The snapshot bench in the harness at 100, 3000 and 30000 files, with the arms: copied index, fresh index, copied index plus ignored at-risk files, and the shipped `backup.rs`. Prices 2.5.
- P9. Two writers on a shared common-dir data worktree with the landing gate. Decides the common-dir arm of D2.
- P10. Reftable on git 2.46 or later across the verb rows. Expected to be uneventful, since no hook writes refs.
- P11. LFS push and pull through a real LFS server with the per-namespace remote.
