# Review of the lint primitive consolidation proposal

**Date:** 2026-05-20
**Status:** Review record; revisions tracked at tasks #518, #519, #520
**Scope:** Senior adversarial review of `mock/research/202605202200_lint-primitive-consolidation.md`. Captures every finding with file:line citations so the revision pass can verify each one independently.
**Reviewer framing:** "Be adversarial. The user explicitly asked to poke holes. Find structural defects, hidden coupling, hand-waved problems, missed cases, design no-nos."

## Why this note exists

The proposal at `202605202200_lint-primitive-consolidation.md` claimed seven reusable primitives plus three bespoke would subsume 63 v1 lints. The audit it leaned on read fast and made structural claims that did not hold under line-level verification. This note records what the review caught, what citations support each finding, and what the revision pass must address before the schema design memo can lock anything.

The review verdict was: **fix the load-bearing items, proceed after**. The consolidation insight survives. The primitive count, the dispatch shape, the cache location, and the duplication claim do not.

## Verdict

Five structural defects are load-bearing. They block implementation and they block the schema memo. Four hidden couplings, four missing v1 cases, four design no-nos, and roughly eight open questions are second-tier. They block schema-memo completeness but not its draft.

The framing "code can be naive, viola lands soon" does not rescue the structural items: those are correctness, not performance.

## What the proposal gets right

- The audit insight is real: v1 has shared bugs and duplication across siblings.
- Collapsing token-scan / identifier-pattern / struct-field / fn-signature / content-regex lints into configurable primitives reduces maintained surface.
- Centralising `lint:allow` parsing into `SuppressionMap` (already shipped) is correct.
- Unifying `Lint` + `CrossCrateLint` behind one trait with mode discriminator is plausible (with caveats below).
- Pain-point inventory is honest about the v1 corner cases.

## Structural defects (P0, block implementation)

### 1. AST cache and source-stripping methods cannot live on `Document` as claimed

The proposal contradicts itself. The mockspace-core `Document` trait at `mock/crates/mockspace-core/src/lint.rs:394-399` ships four methods: `path`, `language`, `source`, `content_hash`. The proposal at line 372 says "mockspace-core does not change". At line 366 it lists `Document::ast()` as a mockspace-rs surface. Inside section "AST cache and source views" it shows `fn ast(&self) -> Option<&syn::File>` and `fn source_stripped(&self, opts: StripOpts) -> &str` inside what reads as an `impl Document` block.

Decide which: either the substrate trait grows two methods (breaks the lock claim, forces `syn` into mockspace-core), or the cache lives on concrete `MockspaceDocument` only and the `Lint` trait cannot accept `&dyn Document` for primitives that need AST. The latter forces every AST-using primitive to be generic over a concrete document type or to downcast. The `Box<dyn Lint>` catalog registry will not support either path.

This is load-bearing and unresolved. Pick now; the schema memo cannot lock the trait surface otherwise.

### 2. `LintMode::PerDocument` wraps every document in a fake `Project`

Section "Mode" line 141: "For PerDocument it calls `check` for each Document wrapped in a single-doc Project view." Per the trait at lines 136-138, every PerDocument lint receives `&dyn Project`. For 200 documents × 30 PerDocument lints that is 6000 wrapper Project allocations per run plus 6000 dynamic dispatches.

v1's `check_document(ctx, doc, sink)` shape avoids the wrapping. The proposal collapsed two traits into one signature and inherited the worst-case dispatch cost of the project-scoped half.

Options:
- Restore a `check_document` method on the trait (one trait, two methods, mode selects which the engine calls).
- Accept the per-document wrapper cost honestly. "Naive is fine" does not license gratuitous waste.

Pick one. The user invoked "naive" to license shortcut implementations, not shortcut shape decisions.

### 3. `ForbiddenTokens` cannot subsume both `no-todo` and `no-bare-string`

Reviewer verified line-level:

- `mockspace/lint-rules/src/no_bare_string.rs:35-110` walks tree-sitter `struct_item`/`enum_item` then `field_declaration` nodes, with `is_inside_macro_def` ancestor check and an 8-word-suppression severity-escalation rule.
- `mockspace/lint-rules/src/no_todo.rs:30-50` walks `macro_invocation` nodes, reading `child_by_field_name("macro")`.

These are two different AST traversals over two different node kinds with different exclusion logic. A `positions = ["macro-invocation", "struct-field"]` config field cannot express both without the primitive carrying a node-kind-specific traversal dispatch table.

**The "ForbiddenTokens covers ~19 lints" claim is unverified against the v1 implementations.** Expect this primitive to split during implementation into at least two: `TokenScan` (line-based, no AST) and `AstNodePositionMatch` (tree-sitter node-kind table driven).

The seven-reusable-primitive count is wrong. Expect nine to ten after honest decomposition.

### 4. Push-gate diff base is undefined on detached HEAD

Section "Gate scope and staging" line 162 specifies `git diff --name-only origin/<branch>..HEAD` for push gate. On detached HEAD (CI commonly checks out a commit, not a branch) there is no symbolic branch.

The proposal punts this to "decided per workflow" without naming the fallback. CI integration depends on this. Schema memo must name: full project, or error, or environment-variable fallback (`MOCKSPACE_PUSH_DIFF_BASE`).

### 5. "Drop 8 byte-for-byte duplicates" claim is false

The proposal at line 86 claims arvo's `no_alloc_enforcer.rs` is byte-for-byte identical to stack-lints `NoAlloc`. Reviewer verified:

- `arvo/mock/lints/no_alloc_enforcer.rs:30-86` uses tokens `["Vec<", " String", "Box<"]` and skips `///`/`//!` doc comments. Proc-macro check via `is_proc_macro_crate`.
- `mockspace-hilavitkutin-stack-lints/src/lints/no_alloc.rs:13-33` uses path-prefix matching against approximately 14 fully-qualified `std::collections::*` paths plus an idents list of 12 names. Proc-macro check via `should_skip_proc_macro_source_lint`.

Different tokens. Different exclusion logic. Different proc-macro check. Different scope (arvo: type-position-only; stack-lints: catches import lines).

Migration must consciously pick which behaviour ships per duplicate. "Delete as duplicate" is lossy. **All 8 claimed duplicates need re-audit before deletion**, ideally as part of the next-pass per-lint catalog (below).

## Hidden coupling (P1, schema memo concerns)

### 6. `StructFieldShape` is a strict subset of `FnSignatureShape`

Both walk syn AST, both check a type against a forbidden list, both have visibility filters. The only axis that differs is which AST node carries the type position (field decl vs param/return). Three primitives (StructFieldShape, FnSignatureShape, plus the implicit TypePositionCheck pattern) collapse cleanly into one `AstTypePosition` primitive parametrised by position set.

The proposal ships them split. Three Config types, three test suites, three near-identical AST walkers. **Drop one or two primitives at consolidation.**

### 7. `ContentRegex` cannot ship suggestion-by-token replacements

`vocabulary-discipline` has a dead-term-to-replacement table (`substrate` to `foundations`, `HList` to `cons-list`). A single `regex + message` pattern fires a finding but cannot suggest the right replacement per matched term unless the message is parametrised on the capture, which a `regex: "—"` cannot express because there is no capture.

Either the primitive grows a `replacements: { regex: suggestion }` table, or `vocabulary-discipline` needs a separate primitive. The proposal treats `vocabulary-discipline` as covered; it is not.

### 8. `visibility = "public"` is per-primitive semantic, not unified scope

Section "Path filter" line 118: "`visibility = "public" | "any"` filters items by Rust visibility. Used by primitives 2, 3, 4, 7. Primitive 1 (ForbiddenTokens) ignores it unless a position filter is set." That is the definition of hidden coupling. A user setting `visibility = "public"` on a token-scan lint silently does nothing.

Either reject the field at config validation per-primitive, or move `visibility` into per-primitive config so the schema makes the dependency visible.

### 9. `exempt_categories` couples to per-token decisions that pre-filter cannot honour

v1 `should_skip_proc_macro_source_lint` and `ctx.introduces(primitive)` calls are per-lint decisions. Some lints want the introduction-bearing document but skip specific tokens. Forcing this to a pre-filter (document is either visible or not) drops the finer-grained behaviour without flagging it.

### 10. CLI `--scope` override-vs-intersect is structural, not optional

Open question 9 in the proposal names this. Reviewer flags it is not actually an open question: the answer changes how every consumer's CLI works. Override (whitelist replacement) breaks legitimate per-lint scopes. Intersect (filter down) cannot expand to a wider scope at the CLI. Schema memo cannot lock primitive contracts without locking this.

## Missing cases (P1, v1 features at risk of silent drop)

### 11. Pass-1-walks-all + Pass-2-checks-staged is disallowed

Cross-doc primitives reject `only_staged = true`. A legitimate pattern (collect symbols from all docs in Pass 1, validate only the staged subset in Pass 2) becomes inexpressible. Either `staging_aware` is per-mode-phase (collect: all, check: filtered) or the pattern is consciously dropped.

### 12. LSP/editor surface integration is hand-waved

`RunSurface` at `mock/crates/mockspace-core/src/lint.rs:113` has three values: `Local`, `Ci`, `Editor`. The proposal does not say which gate the editor surface maps to, or whether editor is a fourth implicit gate. The Gate enum is three-valued and locked.

If editor is a fourth gate, that breaks substrate. If it maps to commit gate, LSP gets `only_staged = true` behaviour which is wrong for "the file being currently edited". Resolution: probably "editor surface ignores staging filter entirely and runs on the active buffer document only". Schema memo names this.

### 13. Per-finding-kind severity overrides dropped silently

v1 supports `[lints.<name>.<finding_kind>] = "severity"`. The proposal has one `default_severity` per lint and gate-level overrides, but no per-finding-kind axis. Verify no v1 consumer depends on this; if any does, migration is lossy. Earlier audit flagged this as G5 (deferred design decision); proposal does not address.

### 14. External lint-pack loading mechanism is unspecified

v1 supports `[lint-crates]` Git dep loading. The proposal mentions stack-lints as a contribution but does not say whether mockspace-rs loads it as a Cargo dep at build time (consumer recompiles to add a lint pack) or as a dynamic plugin at runtime (LINT_CONTRACT_VERSION re-enters scope despite being punted). Schema memo must commit.

## Design no-nos (P2, polish)

### 15. Doc has `substrate` 13 times

Workspace vocabulary rule at `.claude/rules/vocabulary.md` retires the term. Replacement: "foundations" or specific layer name. Discipline-only, not lint-gated, but the word-cloud surfaces it. Revision pass replaces.

### 16. Em-dashes inside fenced code blocks

The doc contains em-dashes inside fenced code blocks at lines 286-287 as regex literal strings demonstrating what `writing-style` matches. Inside a fenced regex this is the literal pattern, which is correct demonstration. The doc outside code fences is em-dash-free per inspection. Acceptable as design-note discipline; flag for awareness.

### 17. Config-validation `Finding` mixes semantic categories

Section "Catalog mechanism" step 3 says bad config produces "a structured Finding emitted before any lint runs". The Finding type carries `rule_id` linking to docs; a config error has no rule. Either Finding grows a `Synthetic` discriminator (semantic pollution) or config errors are a separate `Vec<ConfigError>` and the engine has two output channels. CI workflow behaviour depends on which.

### 18. Catalog `BuiltinKind` enum is closed

`CatalogEntry::kind: BuiltinKind` makes the enum closed. If a consumer wants a new primitive (not just a new instance), they cannot add to the static catalog without recompiling mockspace-rs. Stack-lints contribution is unspecified: either an open registration interface (forces the kind enum open) or stack-lints is special-cased. Schema memo decides.

## Open questions to surface in the schema memo

Beyond the 13 listed in the proposal:

- Where does the AST cache actually live: side-table keyed by `content_hash`, or `RefCell` on concrete document?
- What is the `LintError` type returned by `check`? Proposal shows `Result<(), LintError>` but does not define the error.
- Does `MockspaceDocument` impl `Document` (trait object boxing) or is the engine generic over a concrete Document type? Affects whether `Box<dyn Lint>` can read `doc.ast()`.
- How is parallelism handled? Per-document lints are trivially parallel; project-scoped are not. The proposal does not name a parallelism model.
- What happens when two lints have the same `name` (consumer overrides catalog entry, sibling lint with near-identical name)? Name uniqueness within instantiated list, per-kind, or per-config-block?
- Does `Lint::default_severity()` make sense alongside `[lints.<name>.gate.<g>].severity`? Two sources of truth.
- `proc_macro_crates` derives from Cargo.toml; how does the engine read Cargo.toml? Does mockspace-core provide a CrateGraph, or does the engine re-derive?
- Findings carry `Span`s (substrate) but v1 lints emit line numbers. Migration mapping is not specified.

## Per-finding action map

| # | Severity | Action | Tracked |
|---|---|---|---|
| 1-3, 5, 11, 16 | P0 | Resolve before schema memo; revise proposal | #518 |
| 6-10, 13, 18 | P1 | Surface in schema memo | #519 |
| 12, 13, 14, plus open questions | P1 | Schema memo answers | #520 |
| 15, 17 | P2 | Polish during revision | #519 |

## What the reviewer earned

The review saved a primitive-count error (seven becoming nine to ten during implementation), a deletion-as-duplicate migration error across eight consumer-affecting lints, and a substrate-trait-lock break that would have forced a substrate revision. The audit subagents were faster than the reviewer but read shallower; their structural claims needed verification, and the verifications falsified several.

This is the local-pr-review-flow's intended dynamic. Trust but verify; subagent claims can be wrong even when confidently stated. Workspace rule `feedback_verify_subagent_claims` covers this.

## Next-pass plan

1. **Per-lint AST-shape catalog.** Walk all 63 lints across three pools (mockspace built-ins, stack-lints, per-repo). For each lint: file:line, AST mechanism (line scan / tree-sitter walk / which node kinds), inputs, outputs, config knobs, exclusion patterns. Verify all 8 "byte-for-byte duplicate" claims line by line. Output as `mock/research/<next-timestamp>_lint-corpus-mechanism-audit.md`.

2. **Revise the proposal.** Address #1-3, #5, #11, #16 inline. Re-number primitives based on the catalog's honest count. Acknowledge nine-to-ten reusable plus three bespoke (or whatever the catalog actually shows). Update task #518 with the resolution.

3. **Then start the schema design memo.** Pre-revision, the memo cannot lock load-bearing decisions. Post-revision, it can.

The schema design memo's scope is unchanged; only the inputs are corrected.

## References

- Proposal: `mock/research/202605202200_lint-primitive-consolidation.md`
- Reviewer framing: workspace rule `.claude/rules/local-pr-review-flow.md`
- Sub-agent verification discipline: workspace memory `feedback_verify_subagent_claims`
- Vocabulary discipline (item 15): `.claude/rules/vocabulary.md`

## Recorded

2026-05-20. Senior adversarial review of the consolidation proposal. The next-pass per-lint catalog work begins immediately after this note lands.
