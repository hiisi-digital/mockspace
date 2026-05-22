# Identity: slugs, tasks, refs, content hashes

Mockspace identifies rounds, tasks, and stored content via four
related shapes. Each is a deterministic string (no random suffixes,
no clock dependencies in the identifier itself).

Each identity type lives as a `trait + DefaultX impl` pair so future
swaps (alternative validation, alternative composition, consumer
variants) land as new impls rather than codebase-wide rewrites. The
mockspace canonical impl is the `Default*` variant; consumer code
that wants the default uses the concrete type directly, generic code
parameterises over the trait.

## Slug

A slug is a validated identifier string used for round names, task
leaf names, and namespace segments. The abstraction is the trait
`mockspace-core::slug::Slug`; the canonical impl is `DefaultSlug`
with validation:

- First character: ASCII lowercase letter (`a`-`z`).
- Remaining characters: ASCII lowercase letter, ASCII digit, or
  ASCII hyphen `-`.
- Length: 1 to `MAX_SLUG_LEN` characters inclusive.

Construct via `DefaultSlug::new(s)` (inherent) or
`<DefaultSlug as Slug>::parse(s)` (trait method); generic code uses
`S::parse(s)` where `S: Slug`.

Examples that validate (from the slug test suite):

```
arvo-graph-csr
a
structural-robust-ir
quickstart
round-202605181400
```

Examples that fail:

- `1abc` (digit-leading)
- `-abc` (hyphen-leading)
- `Abc` (uppercase)
- `_abc` (underscore)
- `a_b` (underscore inner)
- `abc.def` (period inner)
- empty string (`DefaultSlugError::Empty`)
- anything longer than `MAX_SLUG_LEN` (`DefaultSlugError::TooLong`)

For round identifiers built around a timestamp, the convention is
to prefix: `round-202605181400` rather than `202605181400`. The
leading-letter rule rejects digit-leading inputs, so naked
timestamps are not valid slugs.

## Namespace

A namespace is a non-empty list of slug segments, joined by `::` in
the wire form. The abstraction is the trait
`mockspace-core::namespace::Namespace` with associated `type Slug`;
the canonical impl is `DefaultNamespace` carrying `Vec<DefaultSlug>`
segments.

Wire form examples:

```
workspace
compiler::ir::lower-pass
```

A namespace with one segment is allowed; a namespace with zero
segments is not (the type is non-empty by construction).

## TaskId

A task is identified by zero or more namespace segments plus a leaf
slug. The abstraction is the trait `mockspace-core::task::TaskId`
with associated types for Slug and Namespace; the canonical impl is
`DefaultTaskId`:

```rust
pub struct DefaultTaskId {
    namespace_segments: Vec<DefaultSlug>,  // may be empty
    slug: DefaultSlug,
}
```

The wire / prose form is `<seg>::<seg>::...::<slug>`, where the
final segment is the leaf slug and any preceding segments form the
namespace. A single segment yields a top-level task with no
namespace.

Examples:

```
migrate-to-codeberg                                # top-level task
workspace::migrate-to-codeberg                     # one-segment namespace
compiler::ir::lower-pass::define-grammar           # three-segment namespace
```

The `#` character is reserved for step references (see `StepRef`)
and is never part of task identity itself.

`DefaultTaskId::parse` errors include:
- `Empty`: input was an empty string.
- `ContainsStepSeparator`: input contained `#`.
- `EmptySegment { position }`: `::` appeared with no content on one
  side.
- `InvalidSegment { index, error }`: a segment failed slug
  validation.

## StepRef

A step within a task is identified by the task ID plus a step slug,
joined by `#`. Per `mockspace-core::task::StepRef`. Wire form:

```
<task-id>#<step-slug>
```

The step slug follows slug validation rules; the task ID portion
follows TaskId parsing.

## RefPath

A `RefPath` wraps a fully-qualified git ref name. The abstraction is
the trait `mockspace-core::ref_path::RefPath`; the canonical impl is
`DefaultRefPath`. Construction stays on the impl side because the
layout (prefix family, segment shape, archive convention) is
impl-specific; the trait only requires `parse` + the supertrait
bundle (`AsRef<str>`, `Display`).

The canonical constructors on `DefaultRefPath` produce:

- `DefaultRefPath::round_mock(slug)` -> `refs/mock/round/<slug>`:
  per-round orphan mock-side ref.
- `DefaultRefPath::round_source(slug)` -> `refs/heads/round/<slug>`:
  source-side branch for a round (regular ref, not orphan).
- `DefaultRefPath::round_conflict(slug, host, timestamp)` ->
  `refs/mock/round/<slug>-conflict-<host>-<timestamp>`: side branch
  written by the conflict-resolution path when a push CAS loses.
- `DefaultRefPath::round_archive()` -> `refs/mock/round-archive`:
  unified closed-rounds archive. Each archived round occupies a
  `<slug>/` subtree.
- `DefaultRefPath::task(namespace, slug)` ->
  `refs/mock/task/<ns-path>/<slug>`: per-active-task orphan ref. The
  namespace path uses `/` separators (`compiler/ir/lower-pass`), not
  `::`.
- `DefaultRefPath::task_archive()` -> `refs/mock/task-archive`:
  unified closed-tasks archive.
- `DefaultRefPath::harness()` -> `refs/mock/harness`: the project's
  configuration ref.

Generic code that needs to construct via the trait still references
`DefaultRefPath`'s constructors because the trait deliberately leaves
construction unconstrained; consumers that want their own layout
implement the trait directly with their own constructor set.

Mockspace stores rounds and tasks under `refs/mock/...` as orphan
refs that don't share ancestry with the consumer's branches. The
source-side companion (`refs/heads/round/<slug>`) is a regular
branch carrying the round's source-side commit history.

## ContentHash

Mockspace records content hashes in two distinct contexts:

- Anchor file entries (`anchor::FileEntry::blob_sha`): hex SHA of
  source-side file content at apply time. Algorithm is whatever the
  source-side git object format uses (SHA-1 today; SHA-256 where
  the consumer's git config selects it). The `BlobSha::parse`
  accepts both lengths (40 hex chars for SHA-1, 64 for SHA-256).
- Manifest verifier records: per-manifest hash declarations live in
  the verifier catalog (`mockspace-core::verifier`); the algorithm
  is named per verifier entry.

## What is identifier vs what is content

| Kind | Allocation |
|---|---|
| `Slug` / `DefaultSlug` | Consumer chooses; validated charset |
| `Namespace` / `DefaultNamespace` | Consumer chooses; segments are slugs |
| `TaskId` / `DefaultTaskId` (segments + slug) | Consumer chooses |
| StepRef step slug | Consumer chooses |
| `RefPath` / `DefaultRefPath` | Derived from slug + namespace, no consumer choice |
| ContentHash | Derived from content, no consumer choice |

Identifiers (slug, namespace, task id, ref path) are stable once
assigned. Content hashes change as content changes. Renaming a
round in place is not supported; the consumer opens a new round
with the desired slug and deprecates the old one.
