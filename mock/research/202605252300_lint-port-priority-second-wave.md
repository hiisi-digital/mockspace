# Lint port priority: the wave after no-todo

**Date:** 2026-05-25
**Status:** Research memo, pre-implementation. Names the order of attack for porting the mockspace built-in lint catalogue onto the cdylib boundary once round 202605251600's no-todo first-port lands.
**Scope:** Task #610 follow-up. Editorial decision: which lint ports next, and after that, and after that. The technical shape is settled in the locked DOC CL; this memo decides which lint each subsequent port-round picks up.
**Source artefacts:**
- `mock/design_rounds/202605251600/202605251600_topic.lint-catalog-cdylib-port.md` (the round committing no-todo as the first-port pattern reference).
- `mock/design_rounds/202605251600/202605251600_changelist.doc.lock.md` (R0 through R5 plus first-port lint scope).
- `mock/research/202605201500_lint-catalog-migration-plan.md` (the 16-built-in / 17-stack-lints / 8-drop / 3-stay-per-repo categorisation).
- `mock/research/202605231400_lint-cdylib-vs-workunit-boundary.md` (why the cdylib boundary is the right shape and what blockers stand between today and the eventual WorkUnit reshape).
- `mock/research/202605252000_cdylib-buffer-ownership-design.md` (PR #213, open at op): the buffer-ownership question gating no-todo's cdylib impl.

## Why this memo now

Round 202605251600 commits scope to one lint: `no-todo`. The first-port pattern reference. The catalogue's remaining 15 built-in lints stay out of scope deliberately so the round closes cleanly once no-todo ships. The follow-on rounds need a clear ordering before they open. This memo proposes that ordering.

The cdylib reimplementation of no-todo is parked on op's A vs B decision in PR #213's buffer-ownership memo. The port-priority question is independent of that decision: regardless of which output-buffer convention wins, the editorial choice of "which lint ports next" stays the same. So this memo can settle today without waiting on PR #213.

## Categorising the 16 mockspace-built-in lints by FFI cost

Each lint's port cost reduces to one question: what data does it need to cross the cdylib boundary to do its job? The NAM v1.0.0 wire schema landed at viola PR #57 carries per-file entries with `path: BytesRef`, `language: arvo::USize`, `source: BytesRef`. That covers raw source bytes for one document. Anything richer needs a NAM schema extension or in-cdylib computation.

Three buckets fall out:

### Bucket 1: pure source-bytes lints (no tree-sitter, single document)

These lints scan one document's source bytes line-by-line or via regex; they do not need an AST. NAM v1.0.0 already covers them. Inside the cdylib: pure regex / line walking. Port cost: minimal, identical to no-todo's shape.

- `file_size`: counts lines, compares against a threshold. The simplest possible body.
- `actionable_errors`: scans for `panic!` / `unwrap` / `expect` / `unreachable!` / `unimplemented!` patterns line-by-line.
- `no_bare_pub`: line-by-line scan for `pub fn` / `pub struct` / `pub enum` without a preceding `pub(crate)` or visibility attribute.

The v2 mockspace-rs ships `ContentRegexLint` at `mock/crates/mockspace-rs/src/builtins/content_regex.rs:124` as the v2 PerDocumentLint surface for regex-shaped lints. The cdylib port of any bucket-1 lint reuses that body shape.

### Bucket 2: AST-shaped lints (tree-sitter, single document)

These lints walk a tree-sitter AST of one document. The NAM v1.0.0 wire schema does not carry parse trees; the cdylib either re-parses inside its own boundary (linking tree-sitter at the cdylib layer) or waits for a NAM schema extension that ships parse-tree entries.

Built-in lints in this bucket:

- `no_todo` (v1 tree-sitter variant in `lint-rules/src/no_todo.rs`; the v1 impl walks `macro_invocation` nodes). Note: the round 202605251600 first-port specifically ports the *regex variant* from `mockspace-rs/src/builtins/content_regex.rs`, not the tree-sitter variant. The two variants give the same answer for compliant code; the tree-sitter variant gives sharper diagnostics for ambiguous cases (e.g. `todo!()` inside string literals). The first port keeps the regex variant; the tree-sitter variant ports later, once the AST-across-NAM question is settled.
- `export_count`: counts pub-export nodes via tree-sitter.
- `no_empty_crate`: checks for substantive content via tree-sitter root walk.

The AST-across-NAM question itself splits into two options: (i) NAM v1.x extension that ships parse-tree entries alongside source bytes, host parses once and shares across cdylibs; (ii) each cdylib re-parses inside its boundary, linking tree-sitter statically. Option (i) saves repeated parse cost at the price of a richer NAM schema; option (ii) keeps NAM small at the price of duplicated parse work. The trade-off is a design conversation for the bucket-2 first-port round; this memo names the question but does not resolve it.

### Bucket 3: cross-crate / project-scoped lints

These lints inspect multiple documents and aggregate per-project state (collision maps, missing-pair detection, etc.). The NAM v1.0.0 wire schema already carries per-file entries as a slice, so iteration across documents is available; what is missing is the cross-document state shape (a `Project` carrier with deduplication-friendly views, name resolution across crates, etc.).

Built-in lints in this bucket:

- `undocumented_type` (CrossCrateLint): tracks type definitions across crates, flags those used externally but not rust-doc-documented.
- `no_duplicate_fn` (CrossCrateLint): tracks function names across crates, flags collisions.

Bucket 3 needs a NAM schema extension for a project-scoped view (or, in the eventual WorkUnit-shaped boundary, a `Project` resource carrying the dedup state). The lift is larger than bucket 1 or 2; this memo flags it as the last wave to port.

## The port-priority order

The proposed order maximises learning velocity per port-round: each round ships a lint that exercises a slightly richer surface than the last, so the boundary shape stays under continuous pressure rather than waiting until bucket 3 to discover NAM schema gaps.

### Wave 1: no-todo (regex variant), in flight under round 202605251600

- Already scoped. Pattern reference for every subsequent port.
- Op decision in PR #213 (A vs B buffer ownership) gates the impl.

### Wave 2: file-size, the simplest bucket-1 candidate

- Trivially mechanical: count lines, compare against a per-pattern threshold from config bytes, emit one finding when over.
- Exercises the per-lint config-bytes pathway end-to-end (no-todo's config is mostly empty; file-size carries a real threshold value, so the wire format for config bytes gets its first real test).
- Same NAM v1.0.0 schema; same DiagnosticBatch fixed-cap; same `mockspace-builtin.lint.<name>.v1` provider id shape. Adds one more `ProviderEntry` to the cdylib's descriptor or ships a sibling cdylib (decision at the time of porting: per-lint cdylib vs per-pack cdylib trade-off documented in the round's R4 resolution).
- Lock criterion: lint runs end-to-end, finding shape matches the v2 PerDocumentLint output, the per-lint config-bytes round-trips correctly.

### Wave 3: actionable-errors and no-bare-pub, multi-pattern regex / line-shape

- Both purely source-bytes lints; same cdylib body shape as no-todo and file-size, but each carries multiple distinct patterns with per-pattern severity overrides.
- Exercises the per-lint config-bytes pathway with a richer schema (list of patterns with options) rather than the single-threshold scalar from wave 2.
- Both port in one round (or two adjacent rounds) since their shape is identical and the only delta is the pattern list.
- Lock criterion: both ports produce findings that match the existing v1 / v2 outputs byte-for-byte for the test fixtures already in the lint-rules crate.

### Wave 4: AST-across-NAM decision round, then no-todo (tree-sitter variant), export-count, no-empty-crate

- This wave opens with a design-only round resolving the AST-across-NAM question (option (i) richer NAM schema vs option (ii) in-cdylib tree-sitter). Once that lands, the three bucket-2 lints port in sequence (or in one combined round if scope stays tight).
- The tree-sitter variant of no-todo ports here, not as a replacement of the wave-1 regex variant but as a sibling registration. Mockspace runs whichever variant the consumer's config selects.
- Lock criterion: each bucket-2 lint produces findings byte-equivalent to its current v1 / v2 output for the fixtures in lint-rules.

### Wave 5: NAM project-scope decision round, then undocumented-type and no-duplicate-fn

- The last wave opens with a design-only round resolving the NAM project-scope shape (cross-document iteration plus per-project dedup state). Once that lands, the two bucket-3 lints port together.
- This wave is the boundary stress test: NAM project-scope is the shape every future cross-crate lint needs, so getting it right pays dividends across the rest of the catalogue.
- Lock criterion: both ports produce findings byte-equivalent to existing v1 / v2 outputs across multi-document fixtures.

## What this memo does NOT lock

- Whether bucket-1 lints ship as one combined `mockspace-builtin-lints` cdylib (one descriptor exporting four `ProviderEntry` rows: no-todo, file-size, actionable-errors, no-bare-pub) or as four separate cdylibs each with one provider. The R4 DOC CL resolution leaned toward per-lint provider ids inside one cdylib for the first-port scope; whether to scale that across the bucket or split into per-lint cdylibs is a per-round call.
- The exact wire format for per-lint config bytes. The first-port round 202605251600 explicitly defers this to the SRC CL slice. Each subsequent port reuses whatever shape the SRC CL lands.
- Whether the wave-4 AST-across-NAM decision picks option (i) or option (ii). The trade-off framing is named above; the decision is the wave-4 design round.
- Whether the wave-5 NAM project-scope shape ships as a separate v1.1.0 schema extension or as a parallel `viola.lint.evaluate.project.v1` vtable. The choice mirrors the R1 NAM-schema-vs-parallel-vtable conversation from round 202605251600's DOC CL, with the same kind of trade-off.
- The 17 stack-lints in `mockspace-hilavitkutin-stack-lints` and the 3 per-repo lints in `arvo/mock/lints/` + `hilavitkutin/mock/lints/`. Those follow the same bucket categorisation and port through the same waves, but they ship from separate repos. Cross-repo coordination is a follow-on memo, not this one.

## Why this matters

The cdylib boundary catalogue port has a known multi-round megaround shape. Without a port-priority memo, each round opens with the same conversation: which lint next, with what shape, under what NAM schema. Locking the order in advance turns that into one decision (this memo, op-confirmable on review), then the subsequent rounds are mechanical against the bucket-by-bucket plan.

The cost of getting the order wrong is small (the buckets are independent enough that a re-prioritisation costs one round of churn, not multi-round redesign). The cost of having no order is repeated indecision at every round opening. This memo trades a small amount of pre-commitment for a steady cadence of port-rounds once no-todo lands.

## See also

- `mock/design_rounds/202605251600/202605251600_topic.lint-catalog-cdylib-port.md` (the first-port round; this memo extends its scope).
- `mock/research/202605252000_cdylib-buffer-ownership-design.md` (PR #213, open at op): the gating decision for wave 1's cdylib impl.
- `mock/research/202605201500_lint-catalog-migration-plan.md` (the 16-builtin categorisation this memo refines).
- Workspace tasks #610 (the parent), #254 (viola becomes a hilavitkutin app; the eventual WorkUnit reshape).
