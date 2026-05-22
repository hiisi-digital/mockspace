# Anchors

An anchor is a content-addressed snapshot of the source-side files
a sealed manifest claimed it would change, taken at the moment the
manifest sealed. Anchors exist so a future `mock phase replan` can
restore the pre-apply state of those files deterministically,
without relying on git history or consumer-side bookkeeping.

## Anchor shape

The persisted form (TOML, at `.anchor.<side>.toml` in the round-ref
tree) carries four top-level fields per the `Anchor` struct in
`mockspace-core::anchor`:

- `mockspace_version`: schema version string for the anchor format.
  Future schema changes bump this.
- `captured_at`: ISO 8601 timestamp when the snapshot was taken.
- `captured_from_source_branch_tip`: source-side branch tip SHA at
  capture, recorded for provenance.
- `file`: a TOML array of per-file entries (serialised as
  `[[file]]`). Each entry carries:
  - `path`: source-side path, relative to repo root, forward-slash
    separated.
  - `blob_sha`: hex SHA of the file's content at capture time.

The blob bodies themselves live at
`.anchor.<side>.blobs/<sha-prefix>/<sha-rest>` for content-addressed
storage (the same content under two different paths shares one
blob).

## Hash algorithms

`BlobSha::parse` accepts both 40-hex-char SHA-1 and 64-hex-char
SHA-256. Which algorithm gets used depends on the source-side git
object format; modern repos default to SHA-1 today, with SHA-256
support per repo configuration.

## When anchors are written

The intended writer is the `apply` transition executor: as part of
sealing the manifest, the executor reads each claimed file's
current content, computes its hash, writes the blob if absent,
records the per-file entry, and writes the anchor TOML alongside
the locked manifest. The executor lives behind
`Transition::Apply` and currently routes through the Phase 5 IO
machinery; see `mockspace-core::transition` for the validity
contract and the in-flight `mock/research/202605220843_phase-5-io-slice-plan.md`
for the implementation slice plan. Earlier slices land the value
shapes; the apply-time anchor capture is the executor's job.

## When anchors are read

A future `replan` reads the anchor of the side being deprecated
and uses the entries to restore source-side files to their
pre-apply state. The replan mode (declared in
`transition::ReplanMode`) controls how restoration handles
post-APPLY work touching claimed files:

- `Destructive`: overwrites claimed files from their anchor blobs.
  Refuses if post-APPLY commits touched any claimed file.
- `AdditiveByCommit`: commits the restoration on top of post-APPLY
  state rather than overwriting.
- `AcceptRestorationLoss(paths)`: accepts post-APPLY work loss for
  the named paths; other claimed paths follow the destructive
  policy.

## What anchors do NOT capture

- Files not claimed by the manifest. A file the manifest did not
  list in its `change` block is not anchored, even if it lives in
  the round's working directory. Restoration leaves those files
  alone.
- The full repository state. Anchors are scoped to manifest claims,
  not to a whole-tree snapshot. If post-APPLY work touched files
  outside the manifest's scope, those edits survive a replan.
- Build artifacts, target directories, anything in `.gitignore`.
  Anchors only capture files the manifest's `change` block names.

## Why anchor instead of git history

Two reasons.

First, the source side may not be in git when apply happens. The
round-ref tree is a separate orphan-ref space; the consumer's
working tree may have uncommitted changes the manifest claims.
Anchors capture the working-tree state, not just the committed
state.

Second, git history changes shape (rebase, force-push). An anchor
is immutable once written. The restoration is identical regardless
of what happens to the source branch between apply and replan.

## What is not yet implemented

The `Anchor` and `FileEntry` value shapes ship today and round-trip
TOML cleanly. The executor that captures and consumes anchors is
in-flight under the Phase 5 IO work; the validity matrix in
`transition::TransitionVerb::next_phase` covers when an apply is
allowed but does not yet bind the I/O sequence. Treat this file's
"when anchors are written / read" sections as describing intent;
the wiring lands progressively.
