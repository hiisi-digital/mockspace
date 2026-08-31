# Anchors

A content-addressed snapshot of the source-side files a manifest claimed,
taken when the manifest sealed, so a later `replan` restores the pre-apply state
deterministically without leaning on git history.

## Shape

TOML at `.anchor.<side>.toml` in the round-ref tree, per `Anchor` in
`mockspace-core::anchor`:

- `mockspace_version`, the anchor schema version
- `captured_at`, ISO 8601
- `captured_from_source_branch_tip`, the branch tip SHA, for provenance
- `file`, an array of `[[file]]` entries carrying `path` (repo-relative,
  forward slashes) and `blob_sha`

Bodies live at `.anchor.<side>.blobs/<sha-prefix>/<sha-rest>`, so one content
under two paths is one blob.

`BlobSha::parse` takes 40-hex SHA-1 or 64-hex SHA-256, whichever the source-side
git object format uses.

## Written on apply, read on replan

The apply executor reads each claimed file, hashes it, writes the blob if
absent, records the entry, and writes the TOML beside the locked manifest.

`replan` reads the anchor of the side being deprecated. `transition::ReplanMode`
decides what post-apply work costs:

- `Destructive`: overwrite from the blobs. **Refuses if any post-apply commit
  touched a claimed file.**
- `AdditiveByCommit`: commit the restoration on top rather than overwriting.
- `AcceptRestorationLoss(paths)`: lose post-apply work on those paths, others
  stay destructive.

## What is not captured

- **Files the manifest did not claim**, even in the round's working directory.
  Restoration leaves them alone.
- **The repository state.** Scoped to claims, never a whole-tree snapshot, so
  edits outside the manifest's scope survive a replan.
- Build output, target directories, anything gitignored.

## Why not git history

**The source side may not be committed at apply time.** The round-ref tree is a
separate orphan-ref space and the working tree may hold uncommitted changes the
manifest claims. Anchors capture the working tree.

**And history changes shape.** Rebase and force-push move it; an anchor is
immutable once written, so restoration is identical whatever happened to the
branch in between.

## Status

**The value shapes ship and round-trip TOML. The executor that captures and
consumes anchors does not.** `transition::TransitionVerb::next_phase` says when
an apply is allowed and does not yet bind the I/O sequence, so read the two
sections above as intent rather than as behaviour.
