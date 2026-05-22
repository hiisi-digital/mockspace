# Phase 5 IO: slice plan against the orphan-ref storage model

`mockspace-core` ships the contract surface for transitions, manifests, anchors, and atomicity. Per spec §19, §24, §25, the storage substrate is git refs (orphan, flat per `refs/mock/round/<slug>`). Phase 5 builds the IO executors against that model. This memo lays out the slice boundaries, the load-bearing primitives each slice introduces, and the open gix-API questions each one resolves before commit.

## Why this memo exists

Tasks #573, #574, #575 (seal_manifest, advance_phase, archive_round) are queued as Phase 5 IO. A filesystem-only stub was attempted earlier in this session as a smaller-blast-radius first step. The user correctly redirected: the v2 design commits to refs, downstream consumers will adopt v2 on cutover, no backwards compatibility shim is needed. This memo replaces the stub plan with the slice sequence against refs.

The slices are sized to one PR each, each landing a primitive that successive slices compose. None of them is a Phase 5 executor in isolation; the executors arrive at slice E and after.

## Dependency choice: gix

`gix` (the pure-Rust git library) is already a workspace dep through `homma` (which uses gix 0.66 for clone/mirror/remote/branch ops). Reusing the same crate in `mockspace-core` keeps the toolchain dep graph coherent. Alternative was libgit2 via `git2`; rejected for build-friction reasons (C dep, OpenSSL link, slower workspace compile).

`gix` is added as a non-optional dependency. Spec §24 does not contemplate a "no-IO" build of mockspace-core; the executors live in the same crate as the contracts.

## Slice sequence

### Slice E1: `mockspace-core` gains `gix` + `RepoHandle`

Add gix to `mockspace-core` Cargo.toml. Introduce a thin `RepoHandle` newtype over `gix::Repository`. One method only: `RepoHandle::open(workspace_root: &Path) -> Result<Self, RepoError>` (walks up from workspace_root to find `.git`, opens via gix).

Surface: `pub struct RepoHandle(gix::Repository)` in a new `io::repo` module. No other ops yet.

Tests: open a tempdir bare repo; assert open() succeeds; assert open() on a path outside any repo returns `RepoError::NotFound`.

Open questions resolved by this slice:
- Does gix's discovery walk find `.git` parent directories cleanly?
- What's the gix-side error shape when no repo is found?
- Does `gix::Repository::open` need extra setup (config, snapshot mode) for orphan-ref reads?

### Slice E2: ref-tree reader

Add `RepoHandle::read_round_ref(&self, slug: &Slug) -> Result<RoundRefTree, ReadError>`. `RoundRefTree` is an in-memory snapshot of the round's orphan-ref tree: a map of leaf-path → blob bytes (`BTreeMap<String, Vec<u8>>`). Internally walks the ref's commit → tree → recursive blob enumeration.

Surface: `RoundRefTree { entries: BTreeMap<String, Vec<u8>> }` with helper readers like `tree.phase()` (returns the `.phase` blob's parsed `Phase`), `tree.manifest(side: ManifestSide) -> Option<Manifest>`, `tree.round_meta()`.

Tests: construct an orphan ref with a known small tree (the test fixtures author it via gix directly); call `read_round_ref` against it; assert the leaf paths and blob bytes round-trip.

Open questions resolved:
- gix's tree-walk API: `gix::Repository::find_reference` → `peel_to_commit` → `tree` → iterate?
- How does gix surface "ref does not exist"?
- Does gix automatically handle the orphan parent-less commit shape?

### Slice E3: ref-tree writer (commit + update-ref)

Add `RepoHandle::write_round_ref(&self, slug: &Slug, new_tree: RoundRefTree, message: &str) -> Result<gix::ObjectId, WriteError>`. Walks the `RoundRefTree` map, hashes each blob into the odb, builds the tree object, builds a new commit (orphan, no parent), updates `refs/mock/round/<slug>` to that commit.

Surface: `WriteError::NonFastForward { current: ObjectId, attempted: ObjectId }` for the CAS case where the ref moved between the read in E2 and this write.

Tests: read a known ref; mutate the tree (insert a new blob, rename one); write back; re-read; assert the mutations round-trip. Concurrent-write test: read twice, write once, second write fails with `NonFastForward`.

Open questions resolved:
- gix odb write API: `gix::Repository::write_blob` / `write_tree` / `write_commit`?
- gix's ref-update CAS: does `update_ref` accept an expected-current-OID for atomic compare-and-swap?
- Author / committer identity: do we need to require gix config to have user.name and user.email set, or do we synthesize "mockspace <noreply@mockspace.local>"?

### Slice E4: `TransitionLock` filesystem impl

Implement the `atomicity::TransitionLock` trait against `.git/mockspace/.lock` using `fs2::FileExt::try_lock_exclusive` or `nix::fcntl::flock`. New module `io::lock` with a concrete `FlockTransitionLock` type. RAII Drop releases the flock.

Surface: `FlockTransitionLock::acquire(workspace_root: &Path, holder: LockHolder) -> Result<Self, LockError>`. The holder's hostname + PID + start time get written into the lock-file body for debugging (per spec §24).

Tests: acquire + drop releases; double-acquire from same process returns `LockError::AlreadyHeld` with the previous holder's identity readable from the file body.

Open questions resolved:
- `fs2` vs `nix`: fs2 is cross-platform (Linux + macOS + Windows) but has a thinner surface; nix is unix-only. Pick fs2 unless its Windows behaviour is broken for our case. (homma does not currently take any lockfile; no precedent to weigh.)
- Lock-file body format: just plain text with three labelled lines, parsed back by the diagnostic reporter on collision.

### Slice E5: anchor capture

Anchor is per-file SHA index for the source-side branch tip captured at APPLY entry. Add `RepoHandle::capture_anchor(&self, source_branch_tip: ObjectId) -> Result<Anchor, AnchorError>`. Reads every blob on the source-side tip (recursively); records each path + blob SHA into an `Anchor` struct (already defined in `anchor.rs`); also writes the blob bytes content-addressed into the anchor-blobs subtree (so anchor restoration does not need network access).

Returns a `BTreeMap<String, Vec<u8>>` shape that can be merged into a `RoundRefTree` by slice E3 to produce the new round-ref tree containing `.anchor.<phase>.toml` + `.anchor.<phase>.blobs/...`.

Open questions resolved:
- gix's tree-walk recursive: does the walker give us paths + blob OIDs efficiently, or do we re-implement?
- Content-addressed blob path: `.anchor.<phase>.blobs/<sha-prefix>/<sha-rest>` where `sha-prefix` is the first 2 hex chars of the SHA (confirmed by spec §23 line 1212 and §25 line 1586).

### Slice E6: `seal_manifest` executor (task #573)

Compose E1-E5 into the seal executor. The function signature roughly:

```rust
pub fn seal_manifest(
    repo: &RepoHandle,
    lock: &FlockTransitionLock,
    slug: &Slug,
    side: ManifestSide,
    source_branch_tip: gix::ObjectId,
) -> Result<SealReport, SealError>
```

The `lock` parameter is borrowed (`&`), not owned: the caller acquires the lock once for the full transition; seal_manifest does not acquire/release internally. The order matches spec §24 step sequence (1-14), with seal scoping covering steps 2, 4, 6, 8, 9, 11. Step 5 (read source-side branch tip SHA) is performed by the caller and passed in as `source_branch_tip`. Steps 7 (verifier) and 10 (render) belong to higher-level orchestration; seal does not run them.

`SealReport` carries the new commit OID + the locked-manifest path so the caller can compose subsequent steps.

### Slice E7: `advance_phase` executor (task #574)

PlanVerb, ApplyVerb, FinishVerb, ReplanVerb each map to a phase-marker rewrite + optional manifest scaffold. Internal to the round's orphan ref; no cross-repo coordination beyond the lock + push.

### Slice E8: `archive_round` executor (task #575)

Moves the round's orphan ref into `refs/mock/round-archive` (a commit-history ref with real lineage), populates `round.toml [closed]`, deletes the per-round ref.

### Slice E9: push CAS + on_phase_race handler

Per spec §24 step 12 + 12a-12e. The push is fast-forward-only against origin. On non-fast-forward: rename local tip to a `refs/mock/round/<slug>-conflict-<host>-<ts>` side branch, push that to origin FIRST (load-bearing per spec), then reset local. This is the trickiest gix work in the sequence; deserves its own slice.

## What lands first

Slice E1 (one PR). Adds the gix dep + opens the door. No semantic surface yet.

E2 → E3 → E4 → E5 → E6 land in sequence. E7-E9 layer on top.

## What's NOT in this plan

- **Worktree machinery for verifier execution** (spec §24 step 7). The verifier runs in `git worktree add --detach`. That's a separate concern from the executors; will land alongside the verifier dispatch work in Phase 5+.
- **Forge API integration** (step 14). Phase 5 ships the on-disk and on-ref work; PR creation lives in the homma forge layer (already shipped for create/migrate/archive).
- **`mock doctor` orphan-ref repair**. Operational concern, not part of the transition critical path.

## Cross-references

- Spec §19 (Reference architecture)
- Spec §24 (Transition atomicity)
- Spec §25 (Active phase storage)
- `mockspace-core::atomicity::TransitionLock` (contract surface, no impl yet)
- `mockspace-core::typestate::TypedManifest::seal` (in-memory transition, no IO)
- homma's existing gix 0.66 usage for clone/mirror/remote/branch (precedent for the dep choice)

## Recorded

2026-05-22 after the user clarified that Phase 5 IO targets the ref-based storage model (no filesystem stubs). An earlier filesystem-only `seal_manifest` was abandoned; this memo documents the slice plan against refs.
