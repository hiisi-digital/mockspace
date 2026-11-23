# Hierarchical row ids in the registry

Branch `work/ids-lane`, cut from `dev` at 3515ae3. Two commits: 8e715e2 (the feature, its tests and
docs) and e59046c (the split of three files that were past five hundred lines). Governing text: the
letkajenkka standard, `SPEC.md` sections 3 to 9, pinned `=0.0.1-rc.1` and unchanged.

## Op's intent

Quoted as briefed, 23 November 2026:

"creating actual hierarchical ids / slugs for all tickets and such. They are long messes of
underscores in prose now. We might want to start namespacing like
ticket::question::engine::internals::deps::allowed. Then querying for ticket::question::engine gives
all engine related tickets. Querying ::internals then gives those. We migrate all domain or such
fields into slugs using any namespaces after their schema kind"

"Remember that everything there is letkajenkka spec, so querying the hierarchical
namespaces/nodes/segments is also globbable and conforms to the spec so things like inline arrays or
whatever syntax in letkajenkka will work on them too."

The intent read: a row id is a path, the ids form a tree a query can walk with every selector the
standard gives a branch, and the fields a namespace classifies by (its kind, its subject) are written
once, as the leading segments of the id.

## What each item became

### 1. A path id and the one place a reference is split

- `RegistryRow.slug` is now `RegistryRow.id`, the whole path as written (`question::engine::allowed`).
  `Resolved::Row.slug` became `id` the same way.
- `row_id_names(id) -> Result<Vec<&str>, String>` and `is_valid_row_id(id)` in `src/ids.rs`: one or
  more plain names of the standard. A hyphen is refused anywhere in an id, though `crate::names`
  takes one in a reference's root.
- `Registry::locate(&self, names) -> Option<RowAt>` in `src/ids.rs`: given the names of a reference,
  with or without `reg` in front, it returns the row and `fields`, the names after its id. It tries
  the prefixes of the names after the namespace, longest first; by item 3 at most one is a row, and
  where a registry carries `id-is-a-branch` the longest wins. `Registry::locate_text` is the same
  over text. The type is `RowAt` because `Located` was taken by `env.rs`.
- Callers moved onto it: `resolve_typed` (the `reg::ns::id[::field]` branch), `pathof`,
  `sourcesof`, `refsto` (through `row_named`, now in `src/resolve_ops.rs`), and
  `find_registry_refs`, which now takes the registry and so the two callers that had one
  (`dangling_references`, `resolve_row`). A reference naming no row is returned whole
  (`ticket::task::loose::nothing`), since without a row nothing says where the id ends.
- Other positional reads removed: `link_target` returns `(namespace, id)` with every name after the
  namespace as the id; `nearest` reads `row.id` and no longer takes `names(q)[1]`; `registry-append`
  takes `<ns>::<path id>`. `grep -rn 'format!("{}::{}"'` over the crate and `src/` now finds only
  joins (`qualified`, `ask.ns::ask.id`), none of them a split. Nothing in the crate splits a
  reference on `::` by hand; the `legacy_refs` reader of the old spelling is the one that did before
  and still does.

### 2. Anchors

`row_anchor(id)` in `src/ids.rs`, used by `RegistryRow::anchor` and by the id cell of a markdown
table (which computed its own before). Each name has `_` written as `-`, and the names are joined
by `.`. A name holds no `-` and no `.`, so the map is one to one: `a::b_c` is `a.b-c` and `a_b::c` is
`a-b.c`; `a__b` (one name) is `a--b` and `a::b` is `a.b`. A one-name id keeps the anchor it had.

### 3. `id-is-a-branch`

`id_branches(reg)` in `src/ids.rs`, called from `validate`, an error with `GateScope::Always`,
naming both rows and both files. The pure core is `id_prefix_pairs(ids)`, over the bare ids of one
namespace, for a tool holding rows its own way. A text prefix that is no path prefix
(`question::engine` and `question::engine_x`) is not a finding, and the same id in two namespaces is
not either.

### 4. Selection

`Registry::select(&self, namespaces, query) -> Result<Vec<&RegistryRow>, SelectRefusal>` in
`src/select.rs`. The registry is read as a branch of namespaces, each a branch of id segments, each id
a record.

- A node stands for the rows at or below it, so `ticket::question::engine` and
  `ticket::question::engine::**` select alike. This is how op's "querying for
  ticket::question::engine gives all engine related tickets" holds.
- Ident, set (by `Path::expand`, written order kept), `*` (byte order of the member names), `**` (the
  node first, then every node below it in byte order, which is byte order of the ids). Each row is
  kept once at its first place over the whole query. The standard says a set's expansion is not
  deduplicated (8.3); what is deduplicated here is the rows selected, which the brief asked for and
  which 8.4 asks of `**`.
- An ident naming nothing under a declared namespace selects nothing; a first step naming no declared
  namespace is refused, since a namespace is declared and a typo in it is not an answer. A namespace
  key with a hyphen (`ikiuni-renderer::*`) is taken as a reference root is.
- Refused, each with its own `SelectRefusal` variant and `say(query)`: ordinal and range (5),
  constraint (no shapes are declared, 9), operation, a query the standard refuses, an expansion past
  a `u64`, and a field step.
- Field steps, my call: the tree is mixed, branches and records side by side, so by 6 the host states
  one behaviour. After a widening step a field step resolves on no row and that row is passed over,
  so `ticket::**::internals::**` is not refused for the rows whose ids end early. A query that has
  not widened names one node, and a step from a record there is refused as `FieldStep` with the row
  named, pointing at `cargo mock query`. `**` from a record is the record itself (zero depth).

### 5. Folding fields into ids

`RegistryNamespace.path: Vec<String>`, the key `path` in `[[registry.namespace]]` (added to
`NAMESPACE_KEYS`). In `src/fold.rs`:

- `fold_row_id(ns, referenced, id) -> Result<Vec<(String, String)>, IdFault>`, pure: the declaration,
  the referenced namespaces' bare ids by namespace key, the row id. It reads the id every way it can:
  each path field in order takes a prefix of the remaining names that is an id of its referenced
  namespace, a field declared `required = false` may also take nothing, and exactly one name must be
  left as the leaf. No reading is `IdFault::NoReading`, several is `IdFault::Ambiguous` with each
  reading, an id that is no path is `IdFault::NotAnId`, an unfit declaration is `IdFault::UnfitPath`.
  This is the rule asked for: a missing optional field is simply not a segment, and ambiguity is
  reported, not resolved.
- `IdFault::kind()` and `IdFault::message(ns, qualified)` give the finding's kind and text, so a tool
  outside the crate reports what the loader reports.
- `unfit_path_fields(namespaces)`, called from `check`: a path naming an undeclared field, a list,
  a scalar, a namespace nobody declared, the namespace itself, or one field twice.
- The loader (`rows_of_texts`) runs `fold_ids` after the rows are read: the derived values go into
  `row.fields`, so readers of `kind` keep working; a row that writes a folded field itself is
  `IdFault::Written`; each fault is kept in `Registry.folds` (`FoldFault`) and `validate` turns them
  into findings. A partial load closes `wanted` over the namespaces the wanted ones fold
  (`with_path_targets`), so a query for `ticket` still reads `kind` and `subject`.
- The generated schema no longer lists a folded field in `required`, and the id and row reference
  patterns are `^[a-z][a-z0-9_]*(::[a-z][a-z0-9_]*)*$`.
- New finding kinds, all in `FINDING_KINDS`: `id-is-a-branch`, `unfoldable-id`,
  `folded-field-written`, `unfit-path-field`.

For the ticket tools in mockspace-extra-lints (crate ticket-model), the public pure functions are
`mockspace_registry::fold_row_id`, `mockspace_registry::row_id_names`,
`mockspace_registry::is_valid_row_id` and `mockspace_registry::id_prefix_pairs`, with `IdFault`
(`kind`, `message`) for the finding. A row writing a folded field is checked there as
`ns.path.contains(field)` for each key the row writes.

### 6. Row reference values

`validate_row_references` takes any row id (`is_valid_row_id`) in a typed field. An item opening on
a declared namespace and naming no row of the field's target is still refused as
`malformed-row-reference` with the old hint (`Write display`), so a qualified value stays refused
rather than read as an id beginning with the namespace's name.

### 7. The command line

`cargo mock select <query>`, beside `query` and `links` in `src/entry/dispatch.rs`, with its help
entry in `src/entry/help.rs` and in the dispatched-commands list of `clean_tests.rs`. It prints one
`namespace::id` a line, nothing with exit 0 on no match, and a refusal on stderr with exit 1.
`cmd_select` lives in `src/select.rs` beside `cmd_query` and `cmd_links`' pattern.

### Docs and builtins

`docs/REFERENCE-SYNTAX.md` (identity, folded ids, selecting rows, links, what is checked),
`docs/USAGE_GUIDE.md` (the command table), the `reference-syntax` skill, the `canon-tiers` skill, the
reference-syntax rule text in `src/render_agent/templates.rs`, and the crate's module docs.

## Tests

New: `src/ids_tests.rs` (9), `src/fold_tests.rs` (10), `src/select_tests.rs` (14) in the registry
crate, `tests/path_ids.rs` (6) in the registry crate, `tests/the_select_verb_lists_rows.rs` (4) in
v1, and one link test in `rowref/link_tests.rs`. Each finding fires on a bad fixture and is silent on
a good one; the selection matrix covers a prefix, trailing `**`, `**` in the middle over zero and
several segments, `*`, a set of two with steps after it, a plain id, a branch id, no match, an
ordinal, a range, a field step, a constraint, an operation, an unknown namespace, and each row once.

Runs on the tip, a crate at a time:

- `cargo test -p mockspace-registry`: 220 lib, 6 `path_ids`, 7 `without_the_engine`; 233 passed, 0
  failed.
- `cargo test -p mockspace registry`: 128 passed. `refs`: 59. `migrate`: 58. `select`: 41. `help`:
  32. `config`: 140. `render_agent` (run before the split, which touches none of it): 212. All 0
  failed.
- `cargo test -p mockspace clean_tests`: 9 passed, 1 failed,
  `entry::clean_tests::suggest_subcommand_declines_far_words` (`frobnicate` suggests `rows-gate`).
  It fails the same with `src/entry/help.rs` as it is on `dev`, so it is not this work; it is
  unfiled, as a helper here files nothing.

## Pre-existing tests changed

- `rowref/link_tests.rs::a_file_citation_is_not_a_link`: it held that three segments are a citation
  whatever the first is called, which was the one-name id assumption. The meaning kept is that a
  citation into a root never reads as a link: the four-segment case now uses a root that is no
  namespace. Said in the test's comment. A collision of root and namespace is still
  `namespace-root-collision`.
- `rowref/link_tests.rs`: three `link_target` assertions compare against an owned id, and the
  `links_report` message reads `namespace::id`.
- `append/tests.rs::the_arguments_are_a_target_a_field_and_a_value`: compares `a.id.as_str()`, as the
  id is now owned.
- `validate_tests.rs`: `PRODUCERS` lists the files that now emit kinds (`ids.rs`, `fold.rs`, and after
  the split `provenance.rs`, `config_keys.rs`, `resolve_data.rs`), and the key lists are imported from
  `config_keys`.
- Every `RegistryNamespace` literal in tests gained `path: Vec::new()`, every `RegistryRow` literal
  `id` for `slug`. No assertion changed with them.

## Left undone

- Migrating existing registries' fields into path ids (op's "we migrate all domain or such fields
  into slugs") is data work in the repositories that hold tickets, and needs a `path` declared there
  first; nothing here rewrites a project's rows.
- `refsto`, `pathof` and `sourcesof` take a path id but not a selection; widening them was not asked.
