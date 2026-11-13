# Mock trees: final consolidation, part two: migrations and test matrix (seat 003)

Part of the canon candidate `017_consolidation.md`, split off along the build's migration seam. Terms, ledger rows (L, D) and residuals (R) are those of that file.

### 4.8 Migration of data trees, with no loss

The rules of 010 2.6 and 014 2.7 stand:
- every step is idempotent;
- snapshot first;
- compare bytes before removing anything;
- verify a remote ref before deleting one;
- `migrate --undo` restores from the copies.

The target changes:
- The root is `.mock/<ns>`.
- Templates leave their crate directories for the shadow tree, at the same relative path (`crates/foo/README.md.tmpl` is `mock://<shadow>/crates/foo/README.md.tmpl`). The move is `git -C <shadow instance> add` of each template, then one git-tree commit that runs `git rm --cached` on every `*.md.tmpl` and lands the `/.mock/` ignore line. The adjacency readers that 001 counted (`012:41`) go through one `shadow(git_path)` function.
- The produced set is seeded for every output: if O equals `Gen(T, C)`, its blob is added; otherwise q starts in conflict state, because an existing hand edit has no baseline (`016:72`).
- In the shipped v2, `refs/heads/mock/<ns>` moves to `refs/mock/<ns>` by the sweep of 4.9, and pending commits and `refs/mock-backup/*` are made ref-held first (014 2.7).

### 4.9 Migration of the registry stores

These steps are 014 2.8 unchanged:
- R.0: snapshot, plus a `git bundle` per store into the user cache.
- R.1: fetch `refs/heads/mock/<ns>` and merge it through the store's hook.
- R.2: publish to `refs/mock/<ns>` unforced, and verify with `ls-remote`.
- R.3: delete `RemoteForm::Branch` and `branch_form` (`mock/crates/mockspace-store/src/refs.rs:56-62`); `fetch_refspec` (`:73-77`) loses its `+`; the `form` key of `RefDecl` (`mock/crates/mockspace-registry/src/stores.rs:286-310`) loses `branch`.
- R.4: delete the remote branch only once its tip is an ancestor of the published ref.
- R.5: the sweep. Any `refs/heads/mock/<ns>` recreated by an old engine is fetched, merged through the store's hook, published, and deleted.
- R.6: prefix-free namespaces across registry and data.

Registry trees take the epoch rule too.

### 4.10 Test matrix, owed red first

Shapes:
- plain;
- opted-in;
- linked;
- bare+linked, with each refspec style;
- mirror;
- each crossed with placement `tree` and `external`.

Verbs: every tree-writing verb listed in 010 2.7, run in the git tree and in the instance.

Pollution rows (now the default exposure, R2):
- a commit tracking `.mock/<ns>/x` arrives by `merge`, `checkout`, `checkout -f` and `reset --hard`;
- assert that the edit is recoverable from the pre-write copy or the snapshot, and that `verify` and `pre-push` name it.

Two-way rows:
- every case of `015_probes/j1` and `016_probes/p1`;
- the classification and base rows of `017_probes/k1`;
- the driver rows of `017_probes/k2`, run through the real generated driver;
- regeneration repeated with an edit pending (N1, N2);
- a squash merge, a cherry-pick and a direct commit as the source of an edit;
- a GitHub-style merge with no custom driver.

Purge rows: clones A to E of `016_probes/p2`, plus the witness carried only by `Mock-Ref` lines, plus the clone with no contact.

Remote rows: 014 2.9. Registry rows: R.0 to R.6, each interrupted and re-run.

Hook rows: git-shook auto-install over a person's existing hooks and over `core.hooksPath`; the stdin cases of `014_probes/g1b`.

Existing tests: the LFS test inverted to require the pointer through real git-lfs; the reftable test changed to `#[ignore]` with a reason; the four-row test replaced.
