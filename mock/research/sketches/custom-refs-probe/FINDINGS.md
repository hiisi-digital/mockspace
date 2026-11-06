# Custom refs as the home of the mock data: the probe and the first step

Dated 3 November 2026. Written by tooling-track for the job `move_the_mock_directory_and_the_registry_into_custom_refs` and its companion `a_pairing_records_the_commit_time_and_a_git_free_tree_hash`. Op's words are in those two tickets and are not copied here; the evidence for the v2 port as a whole is `research/mockspace-v2-port-evaluation.md` on the kaski channel. This note holds what the probe measured, the shape the first step takes, and what it leaves open.

## What is being asked

The mock directory leaves the code branches in two pieces. The registry becomes one custom ref tree outside any branch, and the rest of the mock directory becomes a second one in the same shape as today. Code stays in the primary tree. On disk an untracked directory instantiates the two trees in the layout they have now, and plumbing maps each instance to its ref the way a worktree is mapped to a branch. Design templates stay beside the code for now (option A), a pin file in the trunk is the banked goal (option B), rounds keep their v1 model, and the probe runs in a scratch repository.

## The probe

`p0-local.sh` in this directory builds a bare remote and a trunk on disk and runs 28 checks, each with its control. The remote leg through the session's proxy is partly done and partly open, below. Every check passed on git 2.43.0.

What it established about the ref trees themselves:

- A parentless commit made with `git commit-tree` over `HEAD:mock/registry` gives a ref whose root is the registry directory. The second tree is built from a temporary index with the trunk's code paths removed (`read-tree`, `rm --cached`, `write-tree`) and keeps the `mock/` shape. No checkout is needed for either.
- `refs/mock/*` is not a branch. `git branch -a` does not list it, `ls-remote --heads` does not list it, and `for-each-ref refs/mock` does.
- A plain `git push origin dev` carries nothing of `refs/mock`. The refs travel only by an explicit refspec (`refs/mock/registry:refs/mock/registry`). The positive control is that the same push does land `dev`.
- A default clone, shallow or full, has no `refs/mock`. A shallow `git fetch --depth 1 origin '+refs/mock/*:refs/mock/*'` brings both refs whole, and one extra `remote.origin.fetch` line makes a plain `git fetch` bring them. So a cloud container, which takes a shallow single-branch clone, needs that fetch line or an explicit fetch in its stand-up, and a fetch that finds nothing must say so rather than read as an empty registry.
- A garbage collection with `--prune=now` keeps every object a custom ref reaches.
- `git update-ref <ref> <new> <old>` is the compare-and-swap, and a stale `<old>` is refused. A push from a stale base is rejected as a non-fast-forward, and `--force-with-lease` with the stale expectation is refused as well. These are the two places a lost race shows, locally and at the remote.

What it established about the instance on disk:

- A separate index file (`GIT_INDEX_FILE=.git/mock-registry.index`) with `--work-tree=.mock/registry` materialises the tree with `read-tree` and `checkout-index -a`, and `add -A`, `write-tree` and `commit-tree -p <old>` commit a change back, covering an added file, an edit and a deletion. This is the mapping without any git admin state for it, and it works in a clone that has the trunk checked out.
- The instance must be excluded (`.git/info/exclude`, or a tracked `.gitignore` line), or the trunk's status shows it untracked. With the exclusion the status is clean.
- A linked worktree also works: `git worktree add --detach .mock/reg-wt refs/mock/registry` checks the ref out, a commit in it moves only the detached head, and the custom ref moves only by an explicit `update-ref`. It lists in `git worktree list`, which the plain-index mapping does not.

Two traps the probe walked into, both worth a guard in the tool:

- A `read-tree` run without `GIT_INDEX_FILE` set writes the ref tree into the trunk's own index. The status then shows every file as added and deleted. The first run of the probe did exactly this. Every call in the tool sets the index explicitly, and the tool refuses when it is unset.
- `git clean -fd` leaves an excluded instance alone and `git clean -fdx` deletes it with its uncommitted work. The same hazard exists for any ignored directory, but an instance holds data that is not anywhere else until it is committed back. The tool commits on a short cycle (every write is a commit to the ref), so an instance holds nothing uncommitted for long, and `never-checkout-over-a-working-tree` is extended to name `clean -fdx`.

The container sets `push.negotiate = true` globally, which makes a push to a `file://` bare remote print `fatal: expected 'acknowledgments', received 'packfile'` and carry on. It is noise and changes no result. The probe turns it off for itself.

## The remote leg

Read through the proxy works for refs outside `refs/heads`. `git ls-remote https://github.com/orgrinrt/mockspace-extra-lints 'refs/*'` lists 20 heads, 64 `refs/pull/*` and 2 tags, and `git fetch --depth 1 <url> '+refs/pull/1/head:refs/mock/probe-pr1'` brings that ref into a local repository with its tree readable. The proxy therefore does not filter by ref namespace on the read side.

Open, and owed by a repository where an outward write is acceptable: a push of `refs/mock/probe` and its fetch back, and a delete of it. No scratch repository exists in the session's scope (`list_repos` for `scratch` is empty, and creating one is outside the session's tools). The DM to coord names the options.

## The shape of the first step

What is in the trunk afterwards, in kaski: `mock/crates`, `mock/applets`, `mock/vendor`, `mock/Cargo.toml`, `mock/Cargo.lock`, `mockspace.toml`, `content/` and the rendered `docs/`. What leaves: `mock/registry` to one ref, and to the other `mock/research`, `mock/benches`, `mock/test-history`, `mock/lints`, `mock/tools`, `mock/agent`, the root templates and tomls, and the closed rounds under `mock/design_rounds`.

The instance location. The cheapest version leaves `mock_dir = "mock"` and instantiates each ref into place (`mock/registry`, `mock/research` and so on), excluded from the trunk by ignore lines. Cargo's workspace at `mock/Cargo.toml` still finds `mock/tools/*` as members, `cargo mock` reads the same paths as today, and nothing in mockspace changes except the loader that fills the instances. Op's `.mock/` spelling is the second choice: it needs `mock_dir` to point at a directory that also holds `crates`, which is a symlink or a path move of 2,528 files. The note recommends in place for step one and keeps `.mock/` as the form the banked pin work moves to, because a rename of the instance root is one config value once the loader exists.

Which data is shared and which is per branch. A ref is shared by every branch and every worktree, so anything whose meaning depends on the branch cannot live in it. The active round files under `mock/design_rounds` are exactly that: the phase gate reads them from the branch's own tree, and a round started on one branch must not open the gate on another. They stay in the trunk. A round's close writes its archive into the rest ref and removes the flat files from the branch in the same landing; an archive left behind by an abandoned branch is harmless, because archives are keyed by the round's timestamp and slug.

How a commit to a ref is made and gated. Every writer (the ticket script, the console, `cargo mock` verbs that write rows, the test-history writer) goes through one verb that takes the instance, runs the registry checks on it, commits onto the ref's tip with a compare-and-swap, pushes with a lease, and on a lost race fetches, merges (the registry already has `registry_merge` and the append driver for exactly this) and tries again. The commit gate over a landing reads the instances at the tips the landing was made against and records them, below.

How a landing is tied to the data it was gated against. This is the pairing the companion ticket binds. A pairing is three keys, not one: the commit hash, the commit's author time, and a git-free digest of the tree (a hash over the sorted paths and file contents, computed by the tool with no git object in it). It resolves by hash first, and when the hash is gone after a rehouse it lists the commits whose time is near the recorded one and matches the digest. The landing commit carries one trailer per data ref (`Mock-Ref: registry <hash> <iso time> <digest>`), and each data commit carries the trunk commit it was made against the same way. Option B's pin file is the same record moved from trailers into a file, so the record is written once, by one function, and both forms read it.

Where the code lives. A `[refs]` table in `mockspace.toml` declares which paths of the mock directory live in which ref, and absent means none, so no consumer changes until it opts in. The verbs (`seed`, `instantiate`, `status`, `commit`, `fetch`, `push`, `verify`) are in the launcher, using `git` plumbing as the probe does. The v2 crates already hold gix-based tree and ref writing for rounds (`io/ref_tree`, `io/ref_write`), and `msg/lib/store.sh` holds the fetch, write and retry in shell; the first implementation follows `store.sh`'s sequence and takes the v2 code only where it saves work, since v2 writes parentless commits and the registry needs history.

## Order

1. The pairing record as a library function and its tests, since both the trailer and the pin read it.
2. The ref verbs on a scratch repository, with the race case, driven by the probe's checks as tests.
3. The loader: the registry and the lint inputs read the instances, and a missing instance is a loud error.
4. mockspace itself first as the consumer (it dogfoods), then homma, then kaski with the cut of the history left alone.
5. The two clones' stand-up gets the fetch line, and the tickets that name `trunk` as where registry changes land (`console.toml:124`, `workflow.toml:222`) go to op as an amendment.

## What is left open

- The remote write leg, until a repository may take a probe ref.
- Whether the closed rounds and the research record go to the rest ref whole or split further. Kaski's `design_rounds` holds 4,959 files in 964 entries and `research` 6,372 files (counted on dev at the time of writing), and a single rest ref makes every research note a commit on a shared tip, which serialises writers that today are independent. The probe cannot answer that; the first measurement of lost-race frequency can.
- The cost of materialising the rest ref in a fresh container: 301.8 MB of research is the bulk, and an instance of it should be sparse or lazy.

## Addendum, 2026-11-03: the store crate already exists, and the first design went wrong without it

The paragraph "Where the code lives" above was written without a search of the repository's crates, and it points at the launcher, `store.sh` and the v2 crates. On mockspace dev there is a ref store already: `mock/crates/mockspace-store`, added in #194 and taken into the v1 workspace in #197. It writes a delta (`Edit`, a put or a remove for each path) as a commit on the tip, pushes with `--force-with-lease`, answers a lost race by fetching again and asking a merge hook what the edit is on the tip that won, retries, and moves a namespace by explicit refspec. Its default remote form is the branch form (`refs/heads/mock/<namespace>` on the remote, `refs/mock/<namespace>` locally), because the cloud proxy accepted a push of `refs/heads/mock/probe` and hung up on `refs/mock/probe` (kaski ticket `the_cloud_git_proxy_refuses_pushes_to_refs_outside_refs`). The remote leg above measured the ref form against a local bare remote, so it holds there and not through the proxy. Ticket `a_probe_for_a_new_store_starts_by_listing_the_crates_that` records the missing search step.

What the first implementation did and why it was refused. A second ref store was written beside it (`DataRef`, 1,800 lines on hiisi-digital/mockspace `work/tooling-track-pairing` at 961e33a), and the senior review (do not merge) reproduced these faults in it, all of which the delta and the store's own git runner remove:

- An instance committed its whole tree onto the ref's current tip, so a stale instance deleted the files of the writer that won. A delta against a recorded base cannot name a path it never held.
- The instance was staged with `git add -A`, which applies ignore patterns, so a file named like an ignored one never reached the ref and nothing said so.
- The exclude line went to the worktree's own git directory, which git does not read for `info/exclude`; the shared exclude file is the common directory's.
- The pairing used the committer time, which a rebase resets. The author time is the one it keeps.

The shape now. The instance is a feature of `mockspace-store` (`Instance`: materialize the tip into a directory, record the base commit in the repository's common directory, `edit` as the delta of the directory against the base, `apply` through the store, `update` to bring the directory to the new tip and refuse a file both sides changed). The store also gained `history`, `reader_of`, `files_of` and `is_shallow`. The pairing stays in mockspace's root crate (`pairing.rs` for the record and the digest, `pairing_ref.rs` for the history and the files of a ref); a `Missing` from a shallow history is reported as `Truncated`, since the commit may only be one that was not fetched. The digest is versioned in its own bytes and has a known-answer test made outside the code.

The order above changes at step 3. The registry in mockspace already builds a `DirStore` or a `RefStore` per namespace from its configuration (`src/registry/stores.rs`), so the loader is read against that before it is planned, and the `[refs]` table of step 4 is the place a namespace chooses its form and its instance directory. The `status` and `verify` verbs, the `Mock-Ref` trailer written at a landing, and the stand-up fetch line are not built yet.
