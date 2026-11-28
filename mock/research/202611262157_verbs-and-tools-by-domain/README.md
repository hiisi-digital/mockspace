# The verbs and tools, audited by domain

An audit of every built-in subcommand of the engine on `dev` (`dd1366b`, 24 November 2026) and every tool of the extra lints pack a consumer can take beside it, each given a domain, its overlaps, and a call: keep, merge into another, rename, or drop. It plans no round. The round that renames the verbs reads this note first and settles the open choices at the end.

What was read: the command table in `src/entry/help.rs` and its arms in `src/entry/dispatch.rs`, the live listing of `cargo mock tools` on this repository, the options `print_help` lists, the designed surface in `mock/crates/mockspace-cli/src/main.rs`, the verbs the v2 specification names (`mock/research/202605181400_mockspace-v2-spec.md`), and the module docs of each pack tool at the pack's `dev` (`7b92917`). The callers are counted with `git grep` over `dev`, research and design rounds left out, tests left out.

## What binds the calls

These were settled by the maintainer and are not reopened here.

- 23 November: tools and verbs are grouped by the thing they are about, and a grouping by the shape of what a command does is the wrong cut. Overlapping or badly named verbs merge, and a lot of them stop being commands of their own.
- 23 November: a renamed verb keeps no alias; the old name is gone the moment the new one lands.
- 23 November: one search entry point, every query feature a filter of it, the old query tools folded in and removed. `query` stays apart, since it evaluates a reference expression exactly and resolves ids and patterns, where `search` is fuzzy and ranked.
- 23 November: the index under the search covers every file of the repository, research included, and a query never builds it.
- 23 November: a repository is self-contained, and its tools read only it. An outside layer that crosses repositories is optional, and nothing here calls or names it.
- 23 November, and the engine's own catalogue rule: a check is a lint, which gates, or a tool, which reports or makes and never gates.
- 23 November: the same verbs and the same behaviour from the outer layer to the engine to a project, so a domain this engine shares with that layer is named the same in both.
- 24 November: the extra lints pack holds what is not common enough to be built in. Where the plumbing is upstream and specific to one lint or tool, or the thing is general enough that nearly every repository would enable it, it is built in.

## The verbs today

Thirty-six built-in subcommands, the bare run with its options, and one project tool here (`search`, taken from the pack alone). The pack adds seventeen more for a consumer that names its `workflow` feature, and three it registers in every consumer that takes it. The overlaps the list shows before any grouping: `query`, `links` and `search` over the same rows; three ticket tools over one namespace family; `tickets next` and `tackle` both answering what to take next; `canon stats` and `canon-links totals`; four merge drivers and their registrar; five commands that are what a git hook runs; `status`, `activate` and `deactivate` over the hooks while `check` is the readiness report; `test-units` and `test-list` over the tests; `bench migrate` beside `migrate`.

The style the calls use is a noun for the domain and a verb under it, as `bench`, `panel` and `data-refs` already are here.

## Design rounds: `round`

| today | call | reason | callers on `dev` |
| --- | --- | --- | --- |
| `lock` | rename `round lock` | a phase transition of the round, like every verb in this group | 15 files: `README.md`, `docs/USAGE_GUIDE.md`, the changelist lints' messages in `lint-rules/src/changelist_*.rs`, the generated rules and skills in `src/render_agent/` |
| `unlock` | rename `round unlock` | same | 5, the same kinds |
| `deprecate` | rename `round deprecate` | same | 8 |
| `close` | rename `round close` | same | 16, among them `mockspace-core/src/phase.rs` |
| `archive` | rename `round archive` | an abandoned round's close | 4 |
| `round-id` | rename `round id` | the noun is already in the name | 7, among them `lint-rules/src/round_id.rs` and the design-talk skill |
| `rename-round` | rename `round rename` | same | 8, among them the merging skill and `round_merge.rs`'s refusal text |
| `merge-round-meta` | see the merge drivers | a git merge driver over a round's `.meta` | 3: `src/bootstrap/round_driver.rs`, `round_merge.rs`, a test |

The v2 surface names these verbs differently: `phase plan|apply|finish|replan` for the transitions and `round new|list|close|archive` for the rest. That is an open choice, below.

## The registry: `query`, `registry`, `ticket`, `canon`

| today | call | reason | callers |
| --- | --- | --- | --- |
| `query` | keep | the exact evaluation of a reference expression, kept apart from search on 23 November | 5: `docs/USAGE_GUIDE.md`, `lint-rules/src/tool.rs`, three docs in `mockspace-registry` |
| `links` | merge into `query` | it lists what a row links to and what links to it, and its own help says the far end is what `refsto` derives; that is one expression of `query`, exact and unranked | 5: `docs/REFERENCE-SYNTAX.md`, `docs/USAGE_GUIDE.md`, `rowref/link.rs`, the reference-syntax skill, `render_agent/templates.rs` |
| `registry-append` | rename `registry append` | a write of one value into one row; the delegate path `.mockspace/hooks/registry-append` is renamed with it | 5, and the delegate name in every consumer that placed one |
| `merge-registry` | see the merge drivers | | 4 |
| `rows-gate` | see the hooks | | 3 |
| pack `tickets` | merge into `ticket list` and `ticket check` | one domain, the ticket namespaces of the registry, has three top-level tools | none here |
| pack `ticket-add`, `ticket-move` | merge into `ticket add`, `ticket move` | same | none here |
| pack `tackle` | merge with `tickets next` into one `ticket next` | both answer what to take next from the same rows by different rankings; which ranking stays is open | none here |
| pack `finish` | merge into `ticket left <branch>` | it lists the tickets and rows a branch leaves undone beside its own | none here |
| pack `owed` | merge into `ticket owed` | it files a task for every placeholder marker that cites none | none here |
| pack `canon` | keep, as the canon domain's tool | `subjects`, `show`, `forbids`, `contradicts`, `check` are about canon rows and nothing else asks them | none here |
| pack `canon-links` | merge into `canon stats` | `totals` and `canon stats` both count the canon's joins; `rows` and `probes` become views of `stats` | none here |
| pack `parallel-statements` | merge into `canon parallel` | two rows stating one rule is a finding about canon rows | none here |
| pack `intent-or-mechanism` | merge into `canon mechanism` | a row naming an implementation is a finding about canon rows | none here |
| pack `rulings-with-no-verbatim` | merge into `canon unquoted` | a ruling with no words of the person it records is a finding about rows | none here |
| pack `coverage` | merge into `canon coverage` | what reaches each row of a demand namespace, at what authority | none here |
| pack `quote-check` | merge into `canon quotes` | whether rows that quote a document quote it as it is | none here |

Two canon verbs overlap the one search: `canon citers` (who names a tenet) and `canon uncited` are queries over row links, the first exact. By the 23 November rule they are filters, of `query` where exact and of `search --linked` and `--uncited` where ranked, and they go from `canon`. Which of the two takes each is open, below.

## The search and its index: `search`

| today | call | reason | callers |
| --- | --- | --- | --- |
| pack `search` | keep the name; where it is built is open | the one entry point of 23 November, its filters composing | 3: `mockspace.toml`, `lint-rules/src/tool.rs` and its tests |
| pack `research-unbundle` | merge into `research unbundle`, or a `--bundle` input of whatever writes a research note | about research notes, not about the search; the only verb of its domain today | none here |
| `sketches` | rename `sketch list [--open]` | a sketch is the thing; the v2 specification already names `sketch new`, `sketch run` and `sketch report` | 5: `docs/USAGE_GUIDE.md`, the sketching skill, `render_agent/templates.rs` and its test, `src/sketches.rs` |
| `panel` | keep | `seat`, `consolidate`, `status` are a panel's | 10 |

The index this engine reads is its own repository's, and it reads it in this repository alone. Today mockspace takes the tool from the pack's repository over git, alone and without the pack's lints, and the tool pulls the index crate from a third repository. Whether that meets self-containment is a question of where the search is built, which is open below.

## Tests and benches: `test`, `bench`

| today | call | reason | callers |
| --- | --- | --- | --- |
| `test` | keep, its bare form running the suite | the trees the engine owns, rerunning only what moved | 40 files |
| `test-units` | rename `test units [member]` | how tests divide into binaries is about the tests | 6: `docs/USAGE_GUIDE.md`, the testing skill, `render_agent/templates.rs`, `src/suite/fingerprint.rs`, `src/suite/layout_report.rs` |
| `check-test-history` | rename `test history check` | the guard over a test-history record; the hook calls it | 6: `src/bootstrap/hooks.rs`, `history_driver.rs`, the test-history skill, `src/suite/history_*.rs` |
| `merge-test-history` | see the merge drivers | | 5 |
| pack `test-list` | merge into `test list` | every test of every crate, read from the source, beside `test units`; the machinery that reads the suite is built in | none here |
| pack `known-red`, `known-red-set` | merge into `test red` and `test red set` | the catalogue of tests red on the trunk is about tests; two tools over one catalogue | none here |
| `bench` | keep | its own verbs, forwarded whole | 32 |
| `bench migrate` | keep under `bench` | the bench layout's own migration; `migrate` is the repository's shape | in `src/bench.rs` |
| pack `build-weigh` | keep, domain the build | what building one crate costs, read from the target directory; no other verb is about the build | none here |

## Lints and messages: `lint`

| today | call | reason | callers |
| --- | --- | --- | --- |
| `lints` | rename `lint list` | the lints with their severities | 11 |
| `check-message` | rename `lint message` | one message linted against the message policy; the commit-msg and pre-push hooks call it | 3: `docs/USAGE_GUIDE.md`, `lint-rules/src/lib.rs`, the dispatch; and the generated hooks |
| `check-edit` | rename `lint edit` | one lint asked about an edit not yet made; every generated agent hook named after a lint calls it | 6: `src/entry/edit.rs`, `render_agent/builtins.rs`, `content.rs`, `helpers.rs` |
| v2 `explain` | lands as `lint explain` | how one lint resolves through the cascade | `mockspace-cli` only |
| pack `near-copies` | keep, domain the crates' types | close but different types across crates, a report; whether it gates is a lint's question, not this audit's | none here |

The bare run's `--lint-only`, `--commit`, `--strict`, `--scope` and `--doc-only` stay options of the bare run, since the hooks call it so and it is the gate itself.

## The git hooks and merges: `hooks`, `merge`

| today | call | reason | callers |
| --- | --- | --- | --- |
| `status` | rename `hooks status` | it reports only whether the hooks are active | 7, and the v2 `status` reports adoption instead |
| `activate`, `deactivate` | rename `hooks on`, `hooks off`, or keep the verbs under `hooks` | they set and unset the hooks path | 8 and 4: `README.md`, `cargo-mock/README.md`, `src/bootstrap/activate.rs` |
| `gate-report` | rename `hooks report` | run by a hook when its gate was red | 2: `src/bootstrap/hooks/layout.rs`, `src/gate_report.rs`; every consumer's generated hooks |
| `rows-gate` | rename `hooks rows` or `registry gate` | a pre-commit fast path for registry rows alone; by what it is about it is the registry's, by who calls it the hook's | 3: `src/bootstrap/hooks.rs`, `src/entry/rows_gate.rs`, `validate.rs` |
| `merge-registry`, `merge-additions`, `merge-test-history`, `merge-round-meta` | merge into `merge <kind>` | four git merge drivers and nothing else of their domains calls them | 4, 5, 5, 3, and the driver table every clone's git config is written from |
| `register-drivers` | merge into `merge register [--global]` | it writes the same drivers into the user's config | 6 |

A driver rename changes the command git runs in every clone's config, so `merge register --global` rewrites the user's entries and the bootstrap rewrites a clone's on its next run. A clone that has not run since keeps the old command, and with no alias a merge there fails until it does. That is the cost of the no-alias answer here, and it falls once.

## The checkout and its output

| today | call | reason | callers |
| --- | --- | --- | --- |
| `check` | merge into `status` | the readiness of the checkout: git, phase, build, tests, lints; once `status` is free of the hooks it is the word for this | 29 files, many using the word in prose; the readiness report itself in `src/entry/check.rs` and the usage guide |
| the bare run | keep | check, lint and regenerate, which every hook runs | everywhere |
| `clean` | keep | removes generated output | 3 |
| `pdf` | keep, or `docs pdf` if a `docs` domain is made | renders the designs | 1 |
| `migrate` | keep | one named change of the repository's shape | 11 |
| `data-refs` | keep | its verbs already form the family | 13, and the reference-transaction, post-checkout and monitor hooks |
| `tools` | keep | the catalogue of verbs and tools | 16 |
| `help` | keep | | 15 |

## What this leaves

From the thirty-six built-ins and twenty pack tools: `round`, `merge`, `hooks`, `lint`, `test`, `ticket`, `canon`, `registry`, `sketch`, `research`, and the ones kept as they are, `query`, `search`, `status`, `panel`, `bench`, `data-refs`, `migrate`, `clean`, `pdf`, `tools`, `help`, `build-weigh`, `near-copies`. Gone as their own names: `links`, `check`, `lints`, `activate`, `deactivate`, every `merge-*`, `register-drivers`, `gate-report`, `rows-gate`, `check-*`, `round-id`, `rename-round`, `sketches`, `test-units`, and the sixteen pack tools merged into `ticket`, `canon`, `test` and `research`. Nothing is dropped outright: every command has a caller or a hook behind it.

A consumer's own tools under `<mock>/tools/` are that consumer's domain and stay there. The audit asks only that a consumer tool whose domain is one named here becomes a verb of it there, never a second top-level name beside it.

## Where each lives

By the 24 November rule of thumb, applied one tool at a time:

- Built in already, and kept so: every subcommand above.
- Built in by that rule, open only as to timing: `test list` and `test red` (the suite is the engine's), `search` (general, and nearly every repository would enable it), `lint explain`.
- The pack, by the same rule: `ticket`, `canon` and its merged reports, `research unbundle`, `build-weigh`, `near-copies`, since each reads registry namespaces or layouts not every repository declares. Each is open if it turns out nearly every repository enables it.

## Open choices

1. The round's verbs. (a) `round lock|unlock|deprecate|close|archive|id|rename`, the v1 phases kept; (b) the v2 surface, `phase plan|apply|finish|replan` and `round new|list|close|archive`, which also changes the phase model; (c) (a) now and (b) with the v2 cut. (a) changes the names only, (b) the round itself.
2. The merge drivers. (a) one `merge` domain, `merge registry|additions|test-history|round-meta` and `merge register`; (b) each driver under the domain of the file it merges, `registry merge`, `test history merge`, `round merge`, with registration in `hooks` or the bootstrap. (a) keeps the git config in one place, (b) keeps every domain whole.
3. `query` and the registry. (a) `query` stays top level, as the 23 November words name it, with `registry append` beside it; (b) `registry query` and `registry append` under one noun. (b) changes every caller of `query`.
4. `canon citers` and `canon uncited`. (a) both to `query`, exact; (b) both to `search` filters, ranked; (c) `citers` to `query`, `uncited` to `search --uncited`.
5. The ticket domain's noun. (a) `ticket`, the word the registry's namespaces and the rules use; (b) `task`, the v2 surface's word, whose `task new|list|show|start|block|defer|close|move` is designed over the same rows. Either way one of the two goes.
6. Which ranking `ticket next` keeps. (a) `tackle`'s weighing of the canon's signals; (b) the list's own order; (c) the weighing, with the order as a flag.
7. Where `search` is built. (a) into the engine, with the index crate as a declared dependency; (b) kept in the pack, which mockspace takes over git as it does today; (c) its own crate in this repository, which the pack and the engine both depend on. (a) and (c) take mockspace off another repository for its own search.
8. `rows-gate`. (a) `hooks rows`; (b) `registry gate`.
9. A `docs` domain. (a) none, `pdf` and `clean` stay; (b) `docs render|pdf|clean`, with the bare run's regeneration as `docs render`. (b) is the v2 `regenerate`.
10. The order against the shared tool crate. The tool catalogue is to become a crate of its own, shared by this engine and the outer layer. (a) the renames first, in one round, and the crate after; (b) the crate first, and the renames as its first use. Nothing recorded orders them.

## What the searches did not find

`cargo mock search` here, asked in the words of subcommands, rounds, tasks and phases, returned the v2 specification and the May notes on the built-in install surface, and no earlier audit of these verbs. The command list itself is in no index: it came from `cargo mock tools` and the help table.
