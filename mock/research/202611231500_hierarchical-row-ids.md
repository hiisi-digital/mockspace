# Hierarchical row ids in the registry

Branch `work/ids-lane`, cut from `dev` at 3515ae3. Five commits: 8e715e2 (the feature, its tests and
docs), e59046c (the split of three files that were past five hundred lines), ecb5571 (this
report), 7164124 (review pass 1) and the review pass 2 commit at the tip. Governing text: the
letkajenkka standard, `SPEC.md` sections 3 to 11, pinned `=0.0.1-rc.1` and unchanged.

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
  node first, then every node below it, members in byte order of their names step by step; this is not byte order of the joined ids, as `a::b` comes before `a0`, corrected in pass 2 below). Each row is
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
(`kind`, `message`) for the finding. Superseded: a row writing a folded field is checked by
`folded_fields_written` (review pass 1, item 9), and the list grew in both passes below.

### 6. Row reference values

`validate_row_references` takes any row id (`is_valid_row_id`) in a typed field. An item opening on
a declared namespace and naming no row of the field's target is still refused as
`malformed-row-reference` with the old hint (`Write display`), so a qualified value stays refused
rather than read as an id beginning with the namespace's name. Superseded by review pass 1,
item 5: only the target written in front of one of its rows is malformed.

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

## Review pass 1

This section supersedes the account above where they differ: the `select` verb is gone,
folded fields cannot be optional, and selection runs inside resolution.

Op, 23 November, verbatim, on the names between a ticket's kind and its leaf: "Fully arbitrary,
as describes the ticket best."

- A, free names. A namespace with `path` now takes any number of free names after its folded
  fields and before the leaf: with `path = ["kind"]`, `question::engine::internals::deps::allowed`
  has kind `question`, free names `engine::internals::deps` and leaf `allowed`. Free names are
  branches only. Free names make every optional folded field readable two ways, so `path_fields`
  refuses an optional one as `unfit-path-field` and says why. Tests in `fold_tests.rs`: free names
  with one and with two folded fields, no leaf, a first name naming no kind, an optional field
  refused, a two-way reading with required fields, and the pure functions.
- 1. A step naming nothing where nothing widened now fails (`SelectRefusal::NoSuch`). After a
  widening step it yields nothing for that branch. I changed the module doc and the tests that
  locked the old behaviour: `a_step_naming_nothing_where_nothing_widened_fails` and
  `a_wildcard_matching_nothing_is_an_empty_answer`, and in `tests/the_query_verb_takes_a_pattern.rs`
  `a_step_naming_nothing_fails_and_says_why`.
- 2. Ordering. The docs now say byte order of each name, step by step. Fixed in
  `REFERENCE-SYNTAX.md`, the `query` help, the skill, `select.rs` and line 76 above. Fixture `t`
  with `a0` and `a::b`: `t::**` and `t::*` give `[a::b, a0]`.
- 3. `render_markdown_table` anchors through `row_anchor`. Tested on
  `question::engine_x::deps_allowed` (`a_table_anchors_a_path_id_as_its_row_does`).
- 4. `templates.rs` now teaches `<ns>::<id>` everywhere, and a row-reference field "takes the id
  of a row of" its target. A folding namespace's entry gives `ns::<kind>::<subject>::<free
  names>::<leaf>` and says the folded fields are never written. Every other entry says the id is
  one name or several joined by `::`.
- 5. `malformed-row-reference` is kept for one case only: the target namespace written in front
  of the id of one of its rows. Any other valid id naming no row is `unknown-row-reference`.
  Tested with `task` and `ticket` and the value `task::engine::gone` in fields typed `ticket` and
  `task`.
- 6. One reader. `resolve_typed` sends any path that widens, or a plain path naming a branch, to
  `resolve_ops::resolve_selection`, and that calls `Registry::select`. So `{{ }}`,
  `cargo mock query` and data fields all read it alike.
  - What a selection returns: rows of one namespace are that namespace's table holding just
    them, so `.count()`, `.where`, `.select`, `.first` and `.last` narrow it. Rows of several
    namespaces, and fields, are a list.
  - `pathof`, `sourcesof` and `refsto` take a selection, each file, source and referrer once.
  - `resolve_data` resolves a row's selected rows before the row itself (`row_deps`).
  - `find_registry_refs` no longer reports a branch as dangling.
  - Removed: the `select` verb, its help entry and its dispatch, plus `cmd_select`.
    `cargo mock query` takes the pattern, and when a query is refused it prints the refusal's
    reason after "no result".
  - A step from a row names a field exactly when the namespace declares it (`id` included,
    internal fields not). After a widening step, a row whose namespace has no such field is
    passed over. The docs say this host yields rather than fails.
  - Refused: a step past a field (`PastField`).
  - Updated docs: `REFERENCE-SYNTAX.md` (Selecting rows, What the syntax leaves to mockspace),
    `USAGE_GUIDE.md`, the skill and the rule text.
  - Tests: `path_ids.rs` (`a_widening_reference_resolves_through_the_one_reader`,
    `a_branch_resolves_as_its_rows_and_a_missing_row_is_left_and_reported`) and
    `the_query_verb_takes_a_pattern.rs`.
- 7/8. Namespace names are folded only by `mockspace_refnames::fold`. A query must write the
  folded spelling, and the hyphen spelling is a syntax refusal. Two namespaces that fold to one
  name are refused as `Ambiguous` in a query and reported as the new finding
  `namespace-fold-collision`, called from `check`. Row ids are standard names already, so
  nothing in them is folded. `load_registry_for_expr_from` reads a namespace written folded, and
  reads everything for a query opening on a wildcard.
- 9. `after_registry_root` (generic over names and steps) and `names_after_root` in `ids.rs` are
  the one way the root is skipped. `locate`, `refs.rs`, `resolve_ops.rs` and `select.rs` use them.
  `cmd_query`'s column hint now takes the namespace from `crate::root` after `split_methods`.
  - New public pure functions for ticket-model: `folded_fields_written(ns, written_keys)`, and
    `select_ids(query, &[&str]) -> Result<Vec<&str>, SelectRefusal>` over qualified ids. Each
    goes through the same walk as `Registry::select`.
- 10. Tests added or fixed:
  - `the_check_holds_the_path_declaration` (`path_ids.rs`) fails if `check` stops calling
    `unfit_path_fields`.
  - A path field typed as an undeclared namespace is an `unfit-path-field` case.
  - The `refsto` assertion now checks one exact value.
  - The selection tests assert ids, not counts.
- 11. The unreachable arm is gone. `walk` and `from_record` were rewritten.
- The public-tree guard. `docs/REFERENCE-SYNTAX.md` and the skill named a private project in
  their namespace example. It is now `web-renderer`.

Runs on the tip:

- `cargo test -p mockspace-registry`: 232 lib, 8 `path_ids`, 7 `without_the_engine`. All passed.
- `cargo test -p mockspace` with these filters: `registry` 128, `refs` 59, `select` 41,
  `help` 32, `render_agent` 212, `config` 140, `migrate` 58. Also
  `--test the_query_verb_takes_a_pattern` 5, `--test the_public_tree_names_no_private_project` 7,
  `--test the_query_loads_its_own_namespace` 5. All passed.
- `clean_tests`: 9 passed, plus the known red `suggest_subcommand_declines_far_words`.

Changed again by this pass:

- `select_tests.rs` was rewritten for the rules above.
- `fold_tests.rs`: the optional-field test is now the refusal test. The two-way test uses
  required fields, and the pure-function test uses a required subject.
- `path_ids.rs`: subject is required in its config, `task::loose` became
  `task::engine::render::loose`, and the branch test now expects the branch to resolve.
- `tests/the_select_verb_lists_rows.rs` became `tests/the_query_verb_takes_a_pattern.rs`.

## Review pass 2

This section supersedes both accounts above where they differ. Three things changed. `locate`,
`is_branch` and `RowAt` are gone. The hyphen spelling of a namespace is accepted again. Selection
is no longer a reader beside the plain one.

The shared cause of blockers 1, 2 and 3 was two readers, so there is now one. `walk.rs` holds the
only walker of a registry path, and a plain path is a walk that names one member. It reads a
`Store`, which gives the keys in order plus each row's fields and values. Nodes are made only
when a step asks for them. A plain path costs a few map lookups, and a wildcard costs the rows
below the point where it stands. `resolving_row_data_does_not_scale_with_rows_times_registry`
(15,000 rows) stays under its bound.

- 1. Sets that do not widen. Every registry path that is not plain names goes through the
  walker. A walk is `single` when the path does not widen (`Path::widens`, so a set of one does
  not widen) and it reaches one record or one field. Such a walk is shown as a plain read.
  `{{ ticket::{task}::engine::render::split::title }}`, `{{ ticket::{question}::engine }}` (a
  table), `pathof(...)` and `refsto(...)` each resolve as their expansion does. Test:
  `path_ids.rs::a_set_that_does_not_widen_resolves_as_its_expansion`; walker side in
  `walk_tests.rs::a_set_of_one_reads_as_its_member`.
- 2. One spelling rule. `query_text.rs` holds `namespace_named` (a written name and a key fold
  alike) and `standard_spelling`. `standard_spelling` folds the hyphens of the first step after
  the root, and only there, which is dev's 5948066 allowance. The walker, `registry_path`,
  `reference_namespace` (used by `registry_scope::dangling`), `link_target`, `links_report`,
  `find_registry_refs` and `dangling_references` all go through these.
  - The conflict with SPEC 3.2 is stated, not settled, in `query_text.rs`,
    `REFERENCE-SYNTAX.md` (Paths) and the skill.
  - Test: the four spellings of `web-renderer::pass::split` resolve alike, `.count()` agrees in
    both spellings, a misspelled row is dangling in both, and links in either spelling are
    accepted (`path_ids.rs`, `select_tests.rs`, `query_text_tests.rs`).
- 3. Field values. `resolve_ops::render_field` is the one way a field shows: `id`,
  `reference::citation`, typed row references resolved with the unresolved form kept, and plain
  values. Both a plain read and a list of fields go through it. A field the row does not carry is
  `None`: a plain read leaves the reference unresolved, and a list leaves the field out. Test:
  `a_field_read_through_a_wildcard_is_shown_as_a_plain_read_shows_it`.
- 4. A step past a reference field. A typed field, or a link in a `ref` or `ref[]` field, goes
  on into the rows it names (`ticket::*::**::needs::title`). A step past a value field fails with
  `PastField { row, field, ty }`. `unresolved_reason` gives that reason, and the generation now
  passes it to `registry_scope::dangling`, so the `dangling-reference` finding carries it. Tests:
  `walk_tests.rs` (`a_step_past_a_reference_field_goes_on_into_the_rows_it_names`,
  `a_step_past_a_value_field_fails_naming_the_member_and_its_type`), `path_ids.rs`, and
  `registry_scope_tests.rs::the_reason_a_reference_did_not_resolve_is_in_its_finding`.
- 5. Tests that fail if the code is taken out:
  - `path_ids.rs::the_check_reports_two_namespaces_folding_to_one_name` fails if `check` stops
    calling `namespace_fold_collisions`.
  - Longest-wins is now the walker's: a name a longer id takes is that row, and any other name is
    a field of the shorter row. `walk_tests.rs::the_longer_id_wins_a_name_where_one_id_is_a_prefix_of_another`
    fails if the field is tried first.
- 6. A repeated set member is kept, because the set is exactly its expansion. Deduplication now
  happens only within one expansion path, for `**`. `repeated_set_warnings` is printed by
  `cargo mock query` and, for each template, by the generation. It is a warning and gates
  nothing. Tests: `select_tests.rs`, `query_text_tests.rs`,
  `the_query_verb_takes_a_pattern.rs::a_repeated_set_member_is_read_twice_and_said`.
- 7. A row selecting its own branch is one `reference-cycle` finding (they are deduplicated by
  message), and the docs say an epic listing its own branch loops. Test:
  `path_ids.rs::a_row_selecting_its_own_branch_is_one_cycle`.
- 8. `fold_row_id(ns, declared, referenced, id)` takes the declared keys. New
  `id_is_a_branch_message(short, short_at, long, long_at)`, which `id_branches` uses.
- 9. `**` taken from a record covers its fields as well as the record. Taken from a branch it
  reaches records as rows. So `ticket::**::internals::**` now also lists the fields of a row
  whose id ends in `internals`; `select_tests.rs` says so.
- 10. Rustdoc and messages say id: `model.rs`, `refs.rs`, `check.rs`, `rowref/mod.rs`,
  `validate.rs`.
- 11. `after_registry_root` is the one root skip. `peel` in `query_text.rs` is the one place an
  operation is taken off, and `resolve_typed`, `row_deps` and `unresolved_reason` use it.
- 14. `fold_tests.rs` uses `mockspace_manifest::scratch::tempdir`.

Changed by this pass:

- `ids_tests.rs`: the three `locate` tests and the `is_branch` test moved into `walk_tests.rs`,
  whose tests cover the same cases through the walker.
- `select_tests.rs`: the set-overlap test keeps repeats, and the `**` tests account for record
  fields.
- `path_ids.rs`: a `web-renderer` namespace and its rows were added; `dangling_references` now
  takes the namespace declarations.
- `src/registry/mod.rs`: the `dangling_references` call was updated.
- `registry_scope_tests.rs`: `dangling` takes the reason closure.

For ticket-model, the public functions are now:

- `select_ids(query, &[&str])`.
- `fold_row_id(ns, declared, referenced, id)`.
- `folded_fields_written`.
- `row_id_names` and `is_valid_row_id`.
- `id_prefix_pairs` and `id_is_a_branch_message`.
- `namespace_named`, `standard_spelling`, `registry_path` and `reference_namespace`.
- `repeated_set_members` and `repeated_set_warnings`.

Runs on the tip, a crate at a time:

- `cargo test -p mockspace-registry`: 241 lib tests, 14 in `path_ids`, 7 in
  `without_the_engine`. All passed.
- `cargo test -p mockspace` by filter: `registry` 129, `refs` 59, `select` 41, `query` 5, `help`
  32, `render_agent` 212, `config` 140, `migrate` 58, `registry_scope` 11. All passed.
- `--test` runs: `the_query_verb_takes_a_pattern` 7, `the_public_tree_names_no_private_project`
  7, `the_query_loads_its_own_namespace` 5. All passed.
