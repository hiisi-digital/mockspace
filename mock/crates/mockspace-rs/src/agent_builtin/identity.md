# Identity: slugs, tasks, refs, content hashes

Mockspace identifies rounds, tasks, and stored content via four
related shapes. Each is a deterministic string (no random suffixes,
no clock dependencies in the identifier itself).

## The RefTo / NamedRefTo abstraction

Identifier types live as concrete shapes (`Slug`, `Namespace`,
`TaskId`, `RefPath`, `BranchName`) but their **abstraction** lives at
two function-named traits in `mockspace-core::identity`:

- `trait RefTo<T>`, marks that a type acts as a reference to entity
  `T`. Carries no behaviour; pure type-level anchor. Generic code
  that needs "any reference to T" reaches for this trait. Marked
  `#[marker]` (nightly): overlapping impls are sound because the
  trait grants no read-or-write capability that could conflict.
- `trait NamedRefTo<T>: RefTo<T> + AsRef<str> + Display`, adds the
  visible-name surface (parse, display, AsRef). Generic code that
  needs to read, write, or display a reference reaches for this
  trait.

The `T` parameter is the **actual entity** the identifier refers to.
The entity types live in `mockspace-core::entity`: `Round`, `Task`,
`Branch`, `GitRef`, `Instant`. Each identifier impls `RefTo<E>` (and
`NamedRefTo<E>` where the human-name role applies) for the entity it
points at.

The two-trait split captures two distinct **roles** a reference can
play for the same entity:

- **Stable identifier** (`RefTo<T>`), long-lived, machine-keyed,
  composite. Identity is what it IS, not what it spells.
- **Human-readable name** (`NamedRefTo<T>`), short, parseable from
  a single token. The string form IS the canonical name.

For Task, both roles exist: `TaskId` is the stable identifier
(`RefTo<Task>` only), `Slug` is the human-readable name
(`NamedRefTo<Task>`).

## Slug

A slug is a validated identifier string used for round names and
task leaf names. Validation per `mockspace-core::slug::Slug::new`:

- First character: ASCII lowercase letter (`a`-`z`).
- Remaining characters: ASCII lowercase letter, ASCII digit, or
  ASCII hyphen `-`.
- Length: 1 to `MAX_SLUG_LEN` characters inclusive.

`Slug` impls `NamedRefTo<Round>` (human-name role for rounds) and
`NamedRefTo<Task>` (human-name role for tasks). The same validated
shape names either; the trait bound at the use site picks which
kind. Construct via `Slug::new(s)` (inherent) or
`<Slug as NamedRefTo<Task>>::parse(s)` (trait method, with explicit T
disambiguation).

Examples that validate:

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
- empty string (`SlugError::Empty`)
- anything longer than `MAX_SLUG_LEN` (`SlugError::TooLong`)

## Namespace

A namespace is a non-empty list of slug segments, joined by `::` in
URI form and `/` in ref-path form. Per
`mockspace-core::namespace::Namespace`.

**Namespace does not impl `RefTo` or `NamedRefTo`.** It is purely
structural composition of `TaskId`, mockspace does not track "a
namespace" as a distinct entity that could be referenced. The
namespace handling is impl-detail of TaskId composition.

Wire form examples:

```
workspace
compiler::ir::lower-pass
```

## TaskId

A task is identified by zero or more namespace segments plus a leaf
slug. Per `mockspace-core::task::TaskId`. Carries:

```rust
pub struct TaskId {
    namespace_segments: Vec<Slug>,  // may be empty
    slug: Slug,
}
```

`TaskId` impls `RefTo<Task>` ONLY, it is the **stable identifier**
role for the Task entity. Its identity is the composite structure
(segments + leaf), not the joined string. Functions that need the
human-readable name reach for `Slug` (which impls
`NamedRefTo<Task>`); functions that need the composite view reach
for inherent methods (`namespace()`, `slug()`,
`namespace_segments()`).

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

`TaskId::parse` errors include:
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

## RefPath

A `RefPath` wraps a fully-qualified git ref name. Per
`mockspace-core::ref_path::RefPath`. Impls `NamedRefTo<GitRef>` (the
human-name role for git refs).

Canonical constructors (inherent on `RefPath`):

- `RefPath::round_mock(slug)` -> `refs/mock/round/<slug>`: per-round
  orphan mock-side ref.
- `RefPath::round_source(slug)` -> `refs/heads/round/<slug>`:
  source-side branch for a round (regular ref, not orphan).
- `RefPath::round_conflict(slug, host, timestamp)` ->
  `refs/mock/round/<slug>-conflict-<host>-<timestamp>`: side branch
  written by the conflict-resolution path when a push CAS loses.
- `RefPath::round_archive()` -> `refs/mock/round-archive`: unified
  closed-rounds archive.
- `RefPath::task(namespace, slug)` -> `refs/mock/task/<ns-path>/<slug>`:
  per-active-task orphan ref.
- `RefPath::task_archive()` -> `refs/mock/task-archive`: unified
  closed-tasks archive.
- `RefPath::harness()` -> `refs/mock/harness`: the project's
  configuration ref.

Mockspace stores rounds and tasks under `refs/mock/...` as orphan
refs that don't share ancestry with the consumer's branches. The
source-side companion (`refs/heads/round/<slug>`) is a regular
branch carrying the round's source-side commit history.

## BranchName

`BranchName` names a git branch in the consumer's repository. Per
`mockspace-core::branch_name::BranchName`. Impls `NamedRefTo<Branch>`.
Validation follows a practical subset of `git-check-ref-format(1)`.

## Iso8601Utc

`Iso8601Utc` names a point in time as a validated ISO-8601 UTC
timestamp string. Per `mockspace-core::iso8601::Iso8601Utc`. Impls
`NamedRefTo<Instant>`. Format: `YYYY-MM-DDTHH:MM:SSZ`, second
resolution, no fractional seconds.

Construct via `Iso8601Utc::now()` (system clock) or
`Iso8601Utc::from_unix_secs(secs)` (typed construction from epoch
seconds). The string form parses back via
`<Iso8601Utc as NamedRefTo<Instant>>::parse(s)`. The 1970-9999 range
is the supported domain. The civil-calendar conversion is Howard
Hinnant's algorithm; mockspace ships its own implementation to avoid
a chrono or time dependency for a provenance record.

## ContentHash

Mockspace records content hashes in two distinct contexts:

- Anchor file entries (`anchor::FileEntry::blob_sha`): hex SHA of
  source-side file content at apply time. Algorithm is whatever the
  source-side git object format uses (SHA-1 today; SHA-256 where
  the consumer's git config selects it).
- Manifest verifier records: per-manifest hash declarations live in
  the verifier catalog (`mockspace-core::verifier`); the algorithm
  is named per verifier entry.

## What is identifier vs what is content

| Kind | Allocation | RefTo / NamedRefTo |
|---|---|---|
| `Slug` | Consumer chooses; validated charset | `RefTo<Round>` + `NamedRefTo<Round>`, `RefTo<Task>` + `NamedRefTo<Task>` |
| `Namespace` | Consumer chooses; segments are slugs | (none. purely structural) |
| `TaskId` | Consumer chooses | `RefTo<Task>` only (stable identifier role) |
| `BranchName` | Git's job to allocate | `RefTo<Branch>` + `NamedRefTo<Branch>` |
| `RefPath` | Derived from slug + namespace | `RefTo<GitRef>` + `NamedRefTo<GitRef>` |
| `Iso8601Utc` | Derived from system clock or epoch seconds | `RefTo<Instant>` + `NamedRefTo<Instant>` |
| ContentHash | Derived from content | (none. content addressing, not reference identity) |

Identifiers (slug, namespace, task id, ref path, branch name) are
stable once assigned. Content hashes change as content changes.
Renaming a round in place is not supported; the consumer opens a
new round with the desired slug and deprecates the old one.
