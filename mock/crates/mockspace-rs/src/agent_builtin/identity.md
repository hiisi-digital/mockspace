# Identity: slugs, tasks, refs, hashes

Every identifier is deterministic. No random suffixes, no clock in the
identifier itself.

## The two traits

- `RefTo<T>` marks a type as a reference to entity `T`. No behaviour, pure
  type-level anchor, `#[marker]`.
- `NamedRefTo<T>: RefTo<T> + AsRef<str> + Display` adds the visible name:
  parse, display, `AsRef`.

Entities live in `mockspace-core::entity`: `Round`, `Task`, `Branch`,
`GitRef`, `Instant`.

The split is two roles for one entity. **Stable identifier** is what a thing
is, not what it spells. **Human-readable name** is a short token whose string
form is canonical. Task has both: `TaskId` is the identifier, `Slug` is the
name.

## Slug

First character `a`-`z`. Rest `a`-`z`, `0`-`9`, `-`. Length 1 to
`MAX_SLUG_LEN`. `mockspace-core::slug::Slug::new`.

Impls `NamedRefTo<Round>` and `NamedRefTo<Task>`; the bound at the use site
picks which.

- valid: `graph-csr-backend`, `a`, `quickstart`, `round-202605181400`
- invalid: `1abc` digit-leading, `-abc` hyphen-leading, `Abc` uppercase,
  `_abc` underscore, empty (`SlugError::Empty`), over length
  (`SlugError::TooLong`)

## Namespace

Non-empty list of slug segments. `::` in URI form, `/` in ref-path form.
`compiler::ir::lower-pass`.

**Impls neither trait.** It is structural composition inside `TaskId`, not an
entity mockspace tracks.

## TaskId

Zero or more namespace segments plus a leaf slug. Impls `RefTo<Task>` **only**:
its identity is the composite, not the joined string. Reach for `Slug` when you
need the name, for `namespace()` / `slug()` / `namespace_segments()` when you
need the parts.

Wire form `<seg>::...::<leaf>`. One segment is a top-level task.

```
migrate-to-codeberg                        top-level
workspace::migrate-to-codeberg             one segment
compiler::ir::lower-pass::define-grammar   three segments
```

`#` is reserved for step references and is never part of task identity.

`TaskId::parse` errors: `Empty`, `ContainsStepSeparator`,
`EmptySegment { position }`, `InvalidSegment { index, error }`.

## StepRef

`<task-id>#<step-slug>`.

## RefPath

Wraps a fully-qualified git ref. Impls `NamedRefTo<GitRef>`.

| Constructor | Ref |
|---|---|
| `round_mock(slug)` | `refs/mock/round/<slug>` |
| `round_source(slug)` | `refs/heads/round/<slug>` |
| `round_conflict(slug, host, ts)` | `refs/mock/round/<slug>-conflict-<host>-<ts>` |
| `round_archive()` | `refs/mock/round-archive` |
| `task(namespace, slug)` | `refs/mock/task/<ns-path>/<slug>` |
| `task_archive()` | `refs/mock/task-archive` |
| `harness()` | `refs/mock/harness` |

Rounds and tasks sit under `refs/mock/...` as orphan refs sharing no ancestry
with the consumer's branches. `refs/heads/round/<slug>` is the regular
source-side companion.

## BranchName

Names a branch in the consumer's repository. Impls `NamedRefTo<Branch>`.
Validation is a practical subset of `git-check-ref-format(1)`.

## Iso8601Utc

`YYYY-MM-DDTHH:MM:SSZ`, second resolution, no fractional seconds. Impls
`NamedRefTo<Instant>`. Built by `now()` or `from_unix_secs(secs)`, parsed back
through the trait. Supported domain 1970-9999. Ships its own civil-calendar
conversion rather than take a dependency for a provenance record.

## ContentHash

Two contexts, and the algorithm is named per context rather than fixed:

- `anchor::FileEntry::blob_sha`, the hex SHA of source-side content at apply
  time, in whatever object format that repository's git uses.
- Manifest verifier records, where each verifier entry names its own.

## Identifier against content

| Kind | Allocated by | Traits |
|---|---|---|
| `Slug` | consumer, validated charset | `NamedRefTo<Round>`, `NamedRefTo<Task>` |
| `Namespace` | consumer | none, structural |
| `TaskId` | consumer | `RefTo<Task>` only |
| `BranchName` | git | `NamedRefTo<Branch>` |
| `RefPath` | derived from slug and namespace | `NamedRefTo<GitRef>` |
| `Iso8601Utc` | clock or epoch seconds | `NamedRefTo<Instant>` |
| ContentHash | the content | none, addressing rather than reference |

**Identifiers are stable once assigned; content hashes move with the content.**
Renaming a round in place is not supported: open a new round with the slug you
want and deprecate the old one.
