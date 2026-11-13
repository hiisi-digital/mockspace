# Seat 001 on op's answers to the consolidation

Read: `000_op.md`, section "Answers to the consolidation" (head 06fc104), `010_consolidation.md` (the placement and fsmonitor items, D1 to D4), `011_probes/n1`. Probes for this file are in `012_probes/` (m1, u1, f1, f2, with outputs), git 2.43.0, spikes, no benches. The one timing, f1, is an ad-hoc quick spike and says so in its own header. Terms: I use op's terms from here: git tree for the code tree, mock trees for the data refs together, registry tree where a bespoke name fits.

Gates. Canon gate: op's answers govern and are inside `000_op.md`; nothing here contradicts them. Test gate: the whole suite was run in 001 section 9; nothing in this round changes code.

A leftover to disclose, as the coordinator asked. My first run of f1 hung, and the processes are still alive: the script (pid 28847 and 1269), `git status` (1275), the git-shook dispatcher for `fsmonitor-watchman` (1278) and its `cat` (1292), evidence in `012_probes/f1_hang_evidence.txt`. I could not kill them (refused) and have no stop tool in this seat. They are waiting on stdin forever, which is itself a finding (4.1). They end when the harness stops the background task at its time limit. The committed f1 run is a second copy with `</dev/null`, which finished.

## 1. Placement and naming

### 1.1 What `git~1` is, answered by the probe and one inference

`011_probes/n1_output.txt` (op's own question: the word or the character?): git refuses the whole component `git~1` in any case (`git~1`, `GIT~1`, `Git~1`), and `.git ` and `.git.` (trailing space, trailing dot). It tracks `git~2`, `.git~1`, `~mock`, `~git~1`, `git~1x`, `xgit~1`, `@`, `gitmod~1`, `.gitmodules`. So: neither the tilde nor the word, the exact string. `.git~1` does not work as a guarantee, and `~mock` does not either.

The inference, which I cannot test here, so it is an impression from what the three refused shapes have in common: each is a name that a Windows filesystem treats as an alias of `.git` (`GIT~1` is the 8.3 short name NTFS gives `.git`; the Win32 layer strips a trailing space or dot, turning `.git.` and `.git ` into `.git`). That is why git refuses them in a tree (`core.protectNTFS`, on by default), and it means the refusal and the Windows hazard are one fact. On POSIX a directory called `git~1` at the repository root is an ordinary directory and everything in 008 t9 and 010 c3/c4 applies. On NTFS, a directory called `git~1` (or `.git.`) beside `.git` aliases it: creating it either fails against the existing short name, or opens `.git`, and the worst case writes mock-tree files into `.git`. I have no Windows to test and the engine's store is Unix-only today (`std::os::unix` in `mock/crates/mockspace-store/src/path.rs`, `instance/files.rs`, `instance/stats.rs`, `instance/walk.rs`), so this is a hazard for a future port or for an opted-in Windows clone, not for today. A plain Windows clone never creates the directory (it is only an ignore line there).

`~mock` as a name: git tracks it, there is no guarantee in it, and a shell leaves an unquoted `~mock/f` literal unless a user called `mock` exists (u1 part b: `echo ~mock/f` printed `~mock/f`; the user-exists case is an impression, `getent` printed nothing here). Cross-platform it is fine and worth nothing.

### 1.2 What the refused name still earns, and what it costs

It earns the one thing nothing else in this panel gives: a commit tracking a regular file under the instance root cannot be made by ordinary tools and cannot be written by git in any clone (008 t2 row N). Every other root is exposed to the loss in t2 rows H and T, where a polluting commit destroys the edit silently, plain `merge` included. With op's new terms the question becomes what the root is.

Reading A, the default root is an ordinary configured name, hidden by `info/exclude` and a tracked ignore line, with detection (lint, `verify`, a pre-commit and pre-push refusal in clones that have hooks). Same on every platform, so a path in a doc means the same everywhere. Residue: a polluting commit from a clone with no hooks destroys an edit in the receiving clone, unless the pre-write copy (4) is on. f2 below measures that case.

Reading B, the root is `git~1` where the team is Unix-only and says so in configuration. The guarantee holds there. It makes the instance path differ by platform class, so shared docs cannot spell it.

I recommend A as the default, B as the documented hardening, and op's call on whether "foolproof" outweighs one spelling for every platform. The reason for A is that B's failure on NTFS is the dangerous kind (writes into `.git`), and I cannot test it. If op takes B, the engine refuses to create the root on a platform it cannot establish is not NTFS.

### 1.3 Where the instance root lives, and the `../` paths

There is no literal `mock/` directory. The instance root is configured (`instance_root`), and a mock tree is addressed as `mock://<ns>` by the engine (renderer, references, gates, `Mock-Ref` lines) and as `<instance root>/<ns>` by everything that reads files.

The `../` constraint is a depth constraint, not a name constraint, and I now have its definition. This repository's research data holds paths that leave the mock tree and climb to the repository root (13 of 24 manifests; `mockspace-bench-harness = { path = "../../../../bench-harness" }`), so `<instance root>/<ns>` must sit at the depth `mock/<ns>` did: the root is one component below the top of the worktree. u1 part c: with `instance_root = .mock` and with `git~1`, a sketch at `<root>/research/sketch` with `lib = { path = "../../../crates/lib" }` builds (`cargo metadata` OK in both). A root two components deep fails (005 r1: in-tree one level deeper failed; common dir failed). Rule for the engine: at declaration and at migration, for every file in a mock tree that holds a relative path leaving its namespace directory, the target at the new location must equal the target at the old; the simple form is that the new depth equals the old. That is a check, and it refuses a configuration that would silently break builds.

Per linked worktree: the root is relative to each worktree's top, so each worktree has its own copy (008 section 3; 005 r3). Namespaces that no tool reads by path may sit under the common git directory behind `mock://` (010 D2's remainder); I know of none here (no census).

## 2. The `@` mapping

`@/crates/foo` in a mock tree maps to `/crates/foo` of the git tree. I define it as a pure function both ways: `at(ns, git_path) = <instance root>/<ns>/@/<git_path>` and its inverse `git_path_of(mock_path)`, where the file in the mock tree is a shadow of a git-tree path and its kind is its suffix: `README.md.tmpl` shadows `README.md`, `catalogue.md.tmpl` shadows `catalogue.md`. The renderer asks `at` for the shadow of every git-tree output; tools that want the file read the physical path.

What the mapping replaces, from the source (census, not a measure of effort): the sites that join a crate directory to a template name: `src/render_design.rs:577` and `:582` (`README.md.tmpl`, `DESIGN.md.tmpl` beside the crate), `:503` and `:626` (the `DEEPDIVE_*.md.tmpl` discovery by listing the crate directory), `src/lint/mod.rs:112` and `:471`, `src/registry/mod.rs:615`, `src/document.rs:892` and `:913`, `src/render_agent/content.rs:409`. That is about nine sites, each of which becomes one call to `at`. The discovery sites change shape: today they list the crate's directory, under the mapping they list the shadow directory `<root>/<ns>/@/<crate dir>`. Across `src` the string `.md.tmpl` appears 93 times in 28 files outside tests, most of them tests, parsing and help text; the nine above are the adjacency readers. `src/migrate_refs.rs:261` already collects held templates through `cfg.mock_dir.join(p)`, which is the old coupling.

A caution found by probe (u1 part a): `@` is git's alias for `HEAD`. With a directory named `@` in a worktree, `git diff @` and `git log --oneline @` die with "ambiguous argument '@': both revision and filename", and `git checkout @` quietly switches to `HEAD` (detaching) instead of touching the path. `git diff -- @`, `git add -- @` and `git show @:file` are fine. So a physical directory named `@` at the root of a data worktree is workable and carries three everyday footguns for the person who types in it. The logical `@` stays as op wrote it in `mock://<ns>/@/...` and in docs; the physical directory name should be a configured setting whose default is not a revision alias. Not probed: any tool that treats a leading `@` specially (cargo, editors, a forge UI); impression.

Open for the mapping: relative paths that a shadow file contains and that point at its source neighbours (a template that links `../src/lib.rs`): at the shadow location they resolve to the shadow tree, not the git tree. Nothing I read says whether templates carry such paths; a census of them is owed (6).

## 3. The mirror guard

m1 (`012_probes/m1_output.txt`), file remotes, git 2.43.0. What a `pre-push` hook is handed on `git push --mirror`:

- Case 0, a hook that only logs: the plain clone's mirror push deleted `refs/mock/research` on the remote (`- [deleted] refs/mock/research`) and the hook was handed two lines, both `refs/remotes/origin/*`. "deletion line among them: 0". Git's pre-push hook is not given a deletion that `--mirror` computes, only refs that have a local counterpart. So a hook cannot see, and cannot refuse by name, a mirror deletion.
- Case 2b: with the remote already carrying `refs/remotes/*` from an earlier mirror, the hook is handed 0 lines at all and the deletion goes through (`remote refs/mock after: 0`).
- Case 1: a hook that refuses any pushed line whose remote ref is clone-local (`refs/remotes/*`, `refs/mock-wt/*`, `refs/mock-snapshot/*`, which only a mirror pushes) does stop the first mirror from a clone: rc 1, remote still has the ref. It cannot stop the second one (2b). A heuristic, not a guard.
- Case 3, a rewind: a stale clone's mirror push of an older tip over a newer remote tip. The hook is handed `[refs/mock/research] <old local sha> [refs/mock/research] <newer remote sha>`, can test `merge-base --is-ancestor`, refuses, and the remote tip stays newest. Rewinds and divergences are visible and refusable.

So op's question, "couldn't we just hook to `--mirror` calls": for rewinds, yes; for deletions, no, by git's own behaviour, and a clone with no hook (a clone that never ran mockspace) has nothing at all. A hook cannot be what makes this safe.

The automatic re-push, built on what is visible: in any opted-in clone, every engine step that talks to the remote (`update`, `verify`, the landing gate, `push`) first does one `git ls-remote` of `refs/mock/*` and compares with the local mirror ref: remote absent, create it from the mirror (lease expecting absent); remote behind the mirror, fast-forward it; remote ahead, fetch; diverged, the engine's merge flow. Never a force. Churn: a push happens only when the remote is deficient, it moves no `refs/heads` entry, and a plain clone never sees it; whether a given forge emits webhooks for a custom-ref update is unknown to me (impression). It is automatic and not instant: the window lasts until some opted-in clone runs one of those steps. m1 case 4 shows the healing is monotone: after a stale mirror rewinds the remote, the holder of the newest tip pushes it back as a fast-forward (remote returns to the newest), and the stale clone pushing its old tip again is rejected.

One correction to my own 008 (and a confirmation of 010 D1): the fetch refspec for the mirror ref is unforced. m1 case 4b: a holder with the newest tip fetching a rewound remote: with `refs/mock/<ns>:refs/mock/<ns>` the update is rejected (non-fast-forward) and the local mirror keeps the newest tip; with `+refs/mock/<ns>:refs/mock/<ns>` the local mirror is moved back to the rewound tip and that clone's mirror ref no longer holds the newest. My 008 section 2 wrote `+refs/mock/*:refs/mock/*`. Withdrawn; the unforced form is the build.

The case op asked, that no clone holds the true tip, answered from the probe and from what is not probed:

- The remote still has the objects. After the mirror deleted the ref (case 2) the old tip commit was present on the remote, `fsck --unreachable` lists it (2 commits), and `update-ref refs/mock/research <sha>` restores the ref. They stay until the remote's own gc prunes: case 5, after `gc --prune=now` on the remote the old tip is gone (`could not get object info`). A default git remote keeps unreachable objects for `gc.pruneExpire` (two weeks); what a forge keeps and whether anyone can reach it is not something I can see (impression).
- Every opted-in clone holds the tip under three refs that a remote rewind or deletion does not touch with the unforced refspec: the mirror ref, the working ref of each data worktree, the snapshot ref. A clone that fetched the newest tip once keeps it until someone moves those refs on purpose.
- Every `clone --mirror` holds it (case "clone --mirror" in 008 t7).
- The `Mock-Ref` lines in code-branch history record shas, which locate a tip and recover nothing by themselves.

So the true loss case is narrow and exact: the remote ref gone or rewound, the remote's gc has pruned the old objects, and no clone ever fetched those commits. No git mechanism holds a tip against that, since any ref on the remote that a `--mirror` can delete is deleted; the only protection is on the remote itself (an `update` hook; 008 t8 showed `receive.denyDeletes` does not cover non-heads refs and a hook does), which a forge may not allow. The engine's contribution is to make the window short (re-push on next contact) and to keep the data from being known only to the remote, which is also what op's local copies already do.

## 4. The pre-write copy through git-shook

f1 (ad-hoc quick spike, not a bench: one machine, 30 `git status` per arm, 3000 files of 20 KB, the real git-shook dispatcher at `/home/user/git-shook/git-shook`, git 2.43.0). Mean per `git status`:

- A no fsmonitor: 7 ms.
- B hook direct, protocol floor (answers "everything changed"): 10 ms.
- C behind the git-shook dispatcher, floor: 21 ms.
- D behind shook, a snapshot on every command (copied index, `add -A`, `write-tree`, `commit-tree`, `update-ref`): 71 ms.
- E behind shook, snapshot at most once per 60 s: 27 ms.
- F behind shook, snapshot only if a file under the tree is newer than a stamp: 31 ms.

Per command it is well under a second at this size; the dispatcher alone is about 11 ms of it (a shell, a `rev-parse`, a directory test). The snapshot scales with the number of files git has to stat, not with their bytes, because the copied index carries stat data (003 q3: 18 ms for 3000 files); I extrapolate roughly linearly, which would put 100 000 files in the hundreds of milliseconds, borderline by op's yardstick. That is an extrapolation, not a measurement, and the harness bench with larger trees is the settling probe.

Constant cost: the hook is a program git runs per command; it starts no daemon and leaves nothing running between commands. (The count `hook-related processes: 5` in f1's output is inflated by my earlier hung run, 4.1; it is not the hook's.) This is the cost shape op asked for: paid on git commands only.

What it covers, with f2 added (hook direct, `core.fsmonitor`, the hidden arm of 008 t2): before a polluting `merge`, plain `checkout` and `reset --hard` that destroyed the edit on disk, a copy holding the edit existed in the cache in all three (`cache copies holding the edit: 1`). Together with the audit's a14 (`clean -ffd*`, `reset --hard`, `checkout -f`, `stash -u`) this is every destroyer measured except `worktree remove -f -f` (locked) and `rm -rf`.

### 4.1 A defect in git-shook that blocks shipping it as is

The first f1 run hung. Cause, from the process tree (`f1_hang_evidence.txt`): git runs the `fsmonitor-watchman` hook with git's stdin inherited, the dispatcher replays stdin to its entries with `cat`, and `cat` waits for an end of file that never comes (`git status` -> dispatcher `sh` -> `cat`, 8 minutes and still waiting when I stopped looking). `git-shook` lists `fsmonitor-watchman` among its known events (`/home/user/git-shook/git-shook:246-248`) and replays stdin for every event (`:26-29` says it does so for `pre-push` and `pre-receive`). In a terminal the person's `git status` would wait on the keyboard. The dispatcher must not read stdin for events that do not give any (`fsmonitor-watchman` and most others); with `</dev/null` it works (f1 C to F). That is a fix in git-shook, a prerequisite for ever shipping this, and the probe is the evidence.

Chaining: the dispatcher runs entries in order and stops at the first failure and, for this event, the protocol wants one answer on stdout, so two entries both answering would produce two answers. A person's own fsmonitor entry cannot be chained by running it as a second entry; the mock-tree entry has to be the one that answers, after running the person's and merging their answer. Not probed; impression from the dispatcher's source (`:37-74`).

My reading of op's bar: sub-second per command and no constant cost, which f1 meets at 3000 files; ship on after (a) the stdin fix in git-shook, (b) the chaining answer, (c) the harness bench with trees of 30 000 and 100 000 files and arms D, E, F plus a copy-per-command arm. Until (c) it is "on" in the design and gated by a setting, which is op's "on" with the bench as the exit condition.

## 5. What this changes in my build (008 section 3)

1. The instance root is configured; no `mock/`; `mock://<ns>` for the engine, `<root>/<ns>` for tools; depth rule checked.
2. Default root ordinary, hidden by `info/exclude` and a tracked ignore line, with `git~1` as documented Unix-only hardening (1.2).
3. `@` is a logical name and `at(ns, git_path)` the single function, about nine call sites; the physical directory name configured, default not a revision alias.
4. The mirror ref's fetch refspec unforced. Re-push by contact, never forced. Hooks refuse rewinds and the clone-local-ref signature, and are documented as unable to see mirror deletions.
5. The pre-write copy through git-shook, on, gated by a setting, exit condition the three items in 4.1.

## 6. Open

1. Whether any template or shadow file holds a relative path to a git-tree neighbour (decides whether `@` mapping needs path rewriting).
2. NTFS behaviour of `git~1` beside `.git` and of a root with a leading `.` or `~` on Windows and on case-insensitive APFS: unprobed.
3. What op's hosts keep after a ref deletion, and whether they accept `refs/mock/*` (unchanged from 008).
4. The fsmonitor bench in the harness at 30 000 and 100 000 files, and the git-shook fixes.
5. A webhook or CI trigger on custom-ref pushes: unknown.
6. The census of data that points back into the tree, across consumers.
