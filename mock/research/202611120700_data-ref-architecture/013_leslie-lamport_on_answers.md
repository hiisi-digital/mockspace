# Seat 002 on op's answers: the formal model, the mirror guard, the re-push, the instance root

Read: the end of `000_op.md` ("Answers to the consolidation", op's words), `010_consolidation.md` (skimmed for context, not audited here) and `011_probes/n1_output.txt`. My probes are in `013_probes/` (p1 to p4, scripts with raw output), git 2.43.0 on Linux, `file://` remotes, cargo offline, one cargo at a time, nothing killed. Each is a spike that prices nothing. A real host, Windows and macOS were not tested; where a claim needs one it is marked an impression.

Gates. Canon gate: no `canon_paths`; op's answers govern and I checked each section below against them. I am one reader; a second is owed. Test gate: not re-run, and I say again that I never ran the whole suite.

## 0. Op's questions about the name, answered first

Op asked whether the refusal is the special character or the whole word, whether `.git~1` or `~mock` would work, and how portable it is.

It is the whole alias and not the character. Git's NTFS protection (`core.protectNTFS`, on by default) treats as `.git` the Windows short name of `.git`, which is `git~1`, in any letter case, and `.git` followed by a trailing space or dot, which NTFS ignores. Probe `011_probes/n1` found exactly those refused (`git~1`, `GIT~1`, `Git~1`, `.git `, `.git.`) and `git~2`, `.git~1`, `~mock`, `~git~1`, `git~1x`, `xgit~1`, `@`, `gitmod~1`, `.gitmodules` accepted. My own `009_probes/p2` agrees (it adds `.GIT` refused, and `git~`, `gi~1` accepted), and shows the refusal at every way a commit can bring the path in: `add -f`, `update-index --cacheinfo`, and `checkout -f` of a tree built with `hash-object --literally`. `.git~1` and `~mock` are therefore ordinary names that git does not protect. `~mock` is a legal name; a shell expands an unquoted leading `~mock` as the home directory of a user called `mock` where one exists (impression, not probed). Portability: the refusal is a default of git on every platform, not an NTFS-only behaviour (impression: I ran Linux only); it is switched off by `core.protectNTFS=false` (`009_probes/p2`, section a2: `add -f` then tracks the path).

So only `git~1` (any case) is a directory name that this protection covers, and `.git ` and `.git.` are not directory names a Windows user can make (impression). Section 4 says what the protection is worth.

## 1. The formal model

Terms, as op asked: the git tree is the code tree; mock trees are the data refs together; a bespoke kind may carry its own name, and the registry tree is the one that exists.

Fixed sets. A repository R is an object store with a ref map. A worktree W of R has a root, root(W). Paths are relative to a root, written `/p`, with no leading `mock/` directory anywhere.

Definitions.

1. Git tree. G(W) is the content of W at root(W): tracked files of the code branch checked out in W, plus untracked and ignored files. The code refs are the refs under `refs/heads/`. A path of G is a git-tree path.
2. Mock tree. A mock tree is a tuple M = (ns, kind, tip, instances). `ns` is a name from a prefix-free set (no name is a path prefix of another; `004_probes/p2` shows the ref collision otherwise; the shipped validator admits `/`, `mock/crates/mockspace-store/src/refs.rs:80-`, and `src/data_refs.rs:95-96` refuses only duplicates). `kind` is `data` or a bespoke kind (`registry`). `tip` is a commit of R or none. `instances` is a set of instances, one per worktree that opted in.
3. Refs of a mock tree. Published: `refs/mock/<ns>` on the remote. In an opted-in clone: a dedicated remote `mock-<ns>`, the same URL unless configured otherwise (op: a second repository is an option, not the default); a local forced-fetch mirror; a monotone `seen` ref (section 3); one work ref per worktree, which is where the instance's `HEAD` points as a symref; a cache ref per worktree. None of these is under `refs/heads/` or `refs/tags/`, on the remote or locally.
4. Instance. An instance of M in W is a git worktree of R whose `HEAD` is a symbolic ref to the work ref of (ns, W), physically placed at `root(W)/ρ/<ns>`, where ρ is the instance root.
5. Instance root. ρ is one configured path component, directly under root(W), excluded in the repository's common `info/exclude`, never tracked. It is not named `mock`; that convention is configurable and absent by default.
6. `mock://<ns>/p`. A logical address with two resolutions. Working: `resolve_w(W)` is `root(W)/ρ/<ns>/p`, defined iff W has the instance. Committed: `resolve_c` is the blob at `tip(ns):p`, defined iff the tip exists locally. It is an engine address and is never given to git as a URL: `git ls-remote mock://x/y` is dispatched to a remote helper and fails with "'remote-mock' is not a git command" (`013_probes/p4_output.txt`, last block). That is only a collision with git's helper scheme, harmless while the address stays inside the engine, but the scheme name is claimed by git's own dispatch.
7. `@`. In a mock tree the directory `@/` is the mirror of the git tree. The map φ sends the git-tree path `/q` to the mock-tree path `@/q`; `@/crates/foo/README.md.tmpl` is the template whose output is the git-tree file `/crates/foo/README.md`. φ is injective, so two mock files cannot claim one git path. A path component named `@` is tracked normally (`011` and `013_probes/p4`), but `git log @` and `git diff @` fail as "both revision and filename" and need `--`; so tools address a mock file by its resolved path or put a pathspec after `--`.
8. Registry tree. A mock tree of `kind = registry`: the same ref layout, its own commit and merge rules (the store's merge hook). It exists today as `refs/heads/mock/registry/<store>` (`mock/crates/mockspace-registry/src/test_world.rs:44`, default form branch at `stores.rs:298`), which is inside `refs/heads/` on the remote. Op's ruling is about data refs; whether the registry tree is covered is a question for op, flagged because the model above would put it under `refs/mock/`.

Invariants, each with what establishes it.

- I1, no loss. Every byte a person wrote into an instance is, at every instant, in a file at a path they can name, or reachable from a ref, or in the cache. Established by placement outside every path git can write, snapshots before engine writes, and the repair of section 3. Holds with exceptions, which are the loss paths of `009_probes/p1` (the double-force cleans, `rm -rf`, a polluting commit where the root is not a refused name).
- I2, git tooling unchanged. No git verb is replaced by an engine verb; the engine writes only `info/exclude` lines, the dedicated remote's config, the `refs/mock*` refs and worktree locks; it edits no setting a person owns (`core.bare`, sparse patterns). Established by construction and checked by `verify`; not proved.
- I3, op's ref ruling. No remote ref of a mock tree is under `refs/heads/` or `refs/tags/`; a plain clone, `clone --bare` and `fetch --all` in a plain clone fetch none (`009_probes/p3_output.txt`, first block); `push --all` and `push --tags` publish none (same). Holds on a `file://` remote. A real host is unprobed (`refs.rs:16-19` records one that refused `refs/mock/*`).
- I4, disjointness. ρ is a path no commit of any code ref tracks. Established by a refused name (section 4) where available, else only detected, not prevented.
- I5, monotone publication. The engine's remote writes to `refs/mock/<ns>` are creations or descendants of the tip it last saw; it never deletes and never rewinds. Established by the engine verbs; the repair relies on it (section 3).
- I6, depth stability. For a mock-tree file whose content names a path outside its own instance by a relative path, the instance must sit at the depth where the path was authored; `depth(ρ/<ns>)` from root(W) is fixed, so ρ is exactly one component (section 4).
- I7, address stability. φ is injective, and the mock-to-git direction is generation only: a generation writes tracked git-tree files, and nothing in the git tree names a mock file physically.

## 2. The mirror guard as a state machine

Components. The remote ref r is none or a tip in a chain ordered by ancestry. A clone is one of: plain (no hook, no data), hooked (opted in, `pre-push` installed), unhooked (opted in, the hook displaced by a person's `core.hooksPath`, or bypassed by `--no-verify`). Events: delete the ref, rewind it (a non-fast-forward update), advance it (a fast-forward). Operators: explicit `push --delete`, `push :ref`, forced push, `push --mirror`.

What a `pre-push` hook sees, probed (`013_probes/p1_output.txt`):

| actor and operation | hook runs | hook sees | outcome |
| --- | --- | --- | --- |
| hooked, `push origin --delete refs/mock/x` and `push origin :refs/mock/x` | yes | `(delete) <sha> refs/mock/x <sha>` | refusable; refused, remote unchanged |
| hooked, forced non-ff push to `refs/mock/x` | yes | local and remote sha, ancestry checkable | refusable; refused |
| hooked, `push --mirror`, local `refs/mock/x` stale (identity mapping) | yes | `refs/mock/x <older> refs/mock/x <newer>` | refusable; refused (rc 1, remote kept) |
| hooked, `push --mirror`, the data ref ABSENT locally (tracking-ref mapping) | yes | only the refs the mirror creates or updates (`refs/remotes/*`) | the DELETION of `refs/mock/x` is not on stdin; remote ref gone |
| hooked, `push --mirror` with nothing to create or update | yes, empty stdin | nothing | remote ref gone |
| any, `--no-verify` | no | nothing | remote ref gone |
| plain clone, `push --mirror` | no hook exists | nothing | remote ref gone |
| hooked, `push --all` | yes | heads only | data ref untouched |

So the answer to op's "couldn't we just hook to --mirror calls" is: partly. The hook is called for a mirror push, and it sees and can refuse a rewind of a ref the clone also holds under the same name, and every explicit delete and forced push. It is blind to the deletions a mirror makes (git 2.43, `file://`), it is absent in a plain clone, and `--no-verify` skips it. The plain clone and the displaced hook are exactly where a mirror push is likeliest, so the hook cannot be the guarantee. It is a courtesy that stops the careless cases from hooked clones.

This changes a choice I made in `009` section 2 item 4. I preferred the remote-tracking mapping (`refs/remotes/mock-<ns>/<ns>`) because a deleted remote ref is visible on the next fetch. With the hook in the picture, an identity mapping (the local mirror is named `refs/mock/<ns>`, the shipped engine's local name) is better for opted-in clones: a mirror push from one carries the ref instead of deleting it (`009_probes/p3_output.txt`, identity variant, up to date), and a stale one becomes a rewind that the hook sees and refuses (`013_probes/p1_output.txt`, the S run). The `seen` ref (section 3) stops a forced fetch of a rewound remote from also rewinding the clone's own memory. I withdraw my `009` preference.

Loss, as transitions. Hold(t) is the set of places that keep a tip t reachable: the remote ref, the remote's objects (until the host's gc), every opted-in clone's local mirror, seen, work and backup refs, and the one-day user caches. t is lost iff Hold(t) is empty.

- Remote ref deleted or rewound while any clone holds t: not a loss. The repair of section 3 restores the ref.
- Remote ref gone and the objects still on the remote: recoverable from the id alone. `013_probes/p3_output.txt` (a) and (b): the commit object is still there after the ref is deleted, and a clone that never had the data fetched it by id (`has object after fetch by id: commit`) in this git. The id is on record: every landing carries `Mock-Ref: <ns> <sha>` (design round `202611110830:35`), which makes each landed tip recoverable by id as long as some object store has the object. It depends on the host allowing a fetch by id and on its gc timing; both are unprobed for real hosts.
- Remote ref gone, objects collected by the remote gc, and no clone holds t: lost. `013_probes/p3_output.txt` (a2): after `gc --prune=now` the remote has no such object. That is the answer to op's question, stated plainly: yes, it is lost.

What reaches the lost state: the tip was pushed (so its pusher held it in a work ref and each fetcher in seen), every one of those clones lost its refs together with the day-long cache (a disk loss, or deleting the clone), a destructive push removed the remote ref, and the remote collected the objects. That needs two independent failures; one mirror push in a world with two opted-in clones does not reach it.

Holders a mirror push cannot touch, probed: `receive.hideRefs=refs/mock` on a remote I control. A plain clone's `push --mirror` then leaves `refs/mock/x` in place (`013_probes/p3_output.txt`, block c), and an opted-in clone's push of a new tip is refused ("deny updating a hidden ref"), so it also blocks the engine's own publication. It protects only a remote where publication goes through another channel; it is not a vehicle for a forge. A second repository as a second remote (publish to both) is a second failure domain that a mirror push to `origin` cannot reach, and it is the only holder in this list that adds independence with no server-side configuration. I give it as the answer to "what holds the tip when no clone does" for a project that wants more safety than two clones' worth. Whether to publish to two remotes by default is op's call.

## 3. The automatic re-push

Op: silently re-push the trees unless that causes advancing-head churn; and what if no clone holds the true tip.

Rule, run per opted-in clone at every engine command that already talks to the remote (a fetch, an `update`, a `commit`) and nowhere else. Let r be the remote tip or none, and s the clone's seen tip: the highest remote tip it has ever observed, held in a ref that only ever moves to a descendant of itself.

- r is none and s exists: push s to create the ref. Fire.
- s is an ancestor of r, or equal: set seen to r. No fire.
- r is a proper ancestor of s: push s with `--force-with-lease=refs/mock/<ns>:<r>`. Fire.
- anything else (r and s incomparable): no fire. A concurrent edit; the merge flow owns it.

The engine writes the remote ref in two ways only: a fast-forward or merge publication (the build's verbs), and the repair above. Both are descendants of an observed tip, or a creation (I5).

Proofs, each with the run that exercises it (`013_probes/p2_output.txt`).

1. Safety, repairs never lower the remote tip. A fire pushes s when r is none or a proper ancestor of s, under a lease on r. So every engine transition on the remote moves up the ancestry order or creates, and the engine cannot cause a loss it did not see. Scenarios 2 and 3: a deletion repaired by the first clone, a rewind to c1 repaired to the highest seen tip.
2. Idempotence, no churn in steady state. After a fire r equals s, which is the no-fire case, so repeating the command pushes nothing. Scenario 1: six repair runs over two clones, zero pushes. Scenario 2: after A repaired, B's run and A's second run did not fire.
3. No fight with a legitimate change. A legitimate engine change moves r to a descendant of every tip any clone has seen, so s is an ancestor of r and the rule does not fire (scenario 4). A concurrent divergent edit is incomparable and does not fire (scenario 5b: the remote was rewound to c3 and extended there; the clone's seen tip c4 was incomparable; no push, seen kept). A local unpushed commit does not change s, so it never fires (scenario 6). A move by someone else between the check and the push is stopped by the lease (scenario 7: rc 1, the newer remote tip survived).
4. Bounded repair. Repairs are at most the number of non-engine destructive events: scenario 8, five external rewinds, five repairs, no extra. A periodic external job that rewinds on a schedule draws a repair on that schedule. That is a fight the job started, and the engine should count repairs in a window and stop with a warning past a threshold; the threshold is a policy and unmeasured.

What the proof does not cover. A deliberate rewind by hand (`git push -f` of an older tip) is indistinguishable from a mirror accident and will be repaired. That is the intended direction (no data loss ranks above a purge), but it means a history purge of a leaked secret has no path in the automatic layer: a purge needs an explicit epoch that every clone honours, which I have not designed, and a clone's seen ref would reinstate the secret until told otherwise. It is open, and I do not claim the rule handles it. Also: when r is incomparable to s because someone rewound and extended, the commits of s beyond the common ancestor are no longer on the remote line; the engine warns and keeps them in seen, and publishes no second line.

Op's question again. If a deleted or rewound ref is pushed and no clone holds the true tip, it is lost once the host collects the objects; until then it is recoverable by id from the landing trailers (section 2). The repair makes that state need a second independent failure, and a second remote makes it need a third.

## 4. Where the instance root sits, and what the refused name is worth

Probe `013_probes/p4_output.txt`: a research crate at `<root>/research/sketch` with `lib = { path = "../../../crates/lib" }`, the form the existing manifests have (`006_probes/p1`; the census of 24 manifests, 13 climbing with `../../..`), the data a nested worktree at `<root>/research` in the git tree.

- root `git~1` (one component): cargo from inside the instance resolves.
- root `.mockroot` (one component, ordinary): resolves.
- root `GIT~1`, `.git ` (one component): resolve.
- root `deep/root` (two components): BREAKS.

So the rule is I6: ρ is a single component directly under the git-tree root, because the manifests were authored at depth two (`mock/research/...`) and `<ρ>/<ns>` reproduces depth two. A configured root that is not a single component is a configuration the engine must refuse with the reason; it is the same break `006` found for the out-of-tree placement. Where op's convention goes the other way (a deeper root), the manifests have to be rewritten or generated with the root as a variable, which is a change to the data and not to the engine.

What the refused name is worth under it. With ρ = `git~1`, a commit that brings any path under ρ into a tree is rejected by git: `update-index --cacheinfo` of `git~1/research/N.md` is refused (so are `GIT~1` and `.git `), where it is accepted under `.mockroot` and `deep/root` (`p4`). A crafted commit that does carry the path cannot be checked out: `git checkout` of it fails ("invalid path 'git~1/f'", rc 1) and the instance underneath is untouched. That closes the whole polluting-commit class (`009_probes/p1`, section 3, the dominant in-tree loss) for every namespace at once, by git itself, in a default configuration, and not by a hook one clone can lack. It answers my own 009 limit that only one name is usable: the name is the root, and the namespaces sit below it.

Its limits. `core.protectNTFS=false` turns it off. A commit carrying the path can still enter a repository whose server does not check (`receive.fsckObjects` off), and then every checkout of that branch fails: a denial of service on the branch, not a loss of data. `git add -A` is unaffected because the root is excluded and, in an instantiated clone, the nested worktree is outside the main index (`p4`: `add -f` of a file inside it stages nothing). On Windows the name `git~1` at the top of a git tree sits beside `.git`, whose short name is `git~1`; whether the directory can be created there is something I could not test (impression: it may collide). If it cannot, the protection is unavailable at the depth the manifests need on that platform, and the fallback is an ordinary one-component root with the tracked `.gitignore` line (`009_probes/p2`, section b) and the detective checks, which close the accident and not the deliberate `-f`. An ordinary one-component root keeps the manifests working and loses only the prevention.

## 5. What this leaves open

1. A real host's behaviour for `refs/mock/*`, for a fetch by id of a deleted tip, and for the gc timing that sets the recovery window of section 2. Cheapest test: a throwaway repository on each host op uses.
2. The registry tree's position (section 1, item 8): op's call whether it moves out of `refs/heads/` with the data refs.
3. Windows and macOS: whether `git~1` can exist at the root of a git tree; whether `~mock` is safe in shells.
4. The history purge path that the repair would fight (section 3).
5. The repair's rate limit and the deliberate-rewrite mechanism; how a hook could recognise a mirror at all (it cannot see the deletions, so recognition is by the calling process or `remote.<name>.mirror`, neither probed).
6. The fsmonitor layer's cost, which op bounded at about a second per git command with no constant background cost. I did not measure it here; `009_probes/p4` shows only which verbs call the hook, including plain `git status` and `git diff`, so a cost bench in the harness with a no-hook arm is still owed.
7. Whether to publish to two remotes by default (section 2).
8. The migration from the shipped layout (local `refs/mock/<ns>`, remote `refs/heads/mock/<ns>`, `refs/mock-backup/*`, the pending file) with no loss: still undesigned by me.
