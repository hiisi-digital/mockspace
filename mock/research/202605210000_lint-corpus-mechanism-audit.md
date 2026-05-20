# Lint corpus mechanism audit (per-lint catalog, all three pools)

**Date:** 2026-05-21 (timestamped 0000 to bracket the v2 design-cluster)
**Status:** Verification record; replaces the shallow audit that informed the original consolidation proposal
**Scope:** Line-level read of every v1 lint across the three pools. Verifies the reviewer-flagged shallow-audit errors at `mock/research/202605202300_lint-primitive-proposal-review.md` items #3 and #5. Output drives the revised consolidation proposal (task #518).
**Sibling notes:**
- `mock/research/202605201500_lint-catalog-migration-plan.md` (migration plan; deletion count needs revision)
- `mock/research/202605202200_lint-primitive-consolidation.md` (original proposal; primitive count + duplicate claim wrong)
- `mock/research/202605202300_lint-primitive-proposal-review.md` (reviewer findings)

## Why this note exists

The consolidation proposal claimed seven reusable primitives plus three bespoke would cover the 63-lint corpus, and that eight per-repo lints would drop as byte-for-byte duplicates. The senior reviewer falsified both claims at file:line. This audit walks every v1 lint independently, line by line, with three parallel passes (one per pool). The result corrects the primitive count, the duplicate-drop count, and surfaces a class of lints the proposal's `ContentRegex` primitive provably cannot express.

The audit is the input to proposal revision. Implementation cannot start until the primitive set is honest.

## Headline corrections

1. **Primitive count: ten reusable plus six bespoke**, not seven plus three. Pool A alone needs `AstNodePositionMatch` and `AstTypePosition` as separate primitives (reviewer item #3 verified: they walk different tree-sitter node kinds with different exclusion logic, cannot be unified by config alone). Pool A also has seven lints that flag BESPOKE (cannot fit any primitive). Pool C reveals one additional primitive need (`TermReplacementTable`) that `ContentRegex` cannot cover.

2. **Per-repo duplicate count: five safe drops, three merges, four keepers**, not eight drops. The reviewer's red flag on `arvo/no_alloc_enforcer.rs` was correct, and the parallel verification shows the same pattern across the eight claimed duplicates: most differ at token list, scope filter, or exclusion logic from their stack-lints counterparts.

3. **`vocabulary_discipline` carries a dead-term-to-replacement table** (`chain → fiber`, `partition → phase`, `entity → record`, plus four more). `ContentRegex` as defined in the proposal (regex + message template) has no capture-to-suggestion mechanism. Either the primitive grows a lookup-table extension, or this lint stays bespoke. The proposal's claim that `ContentRegex` covers `vocabulary-discipline` is provably false.

4. **Seven Pool A lints flag BESPOKE**: `deprecation_comparison`, `no_adhoc_framework`, `no_bare_vec` (two-phase AST + macro-body scan), `no_manual_id`, `no_manual_impl`, `registrable_completeness`, plus `forbidden_imports` as soft-BESPOKE (data-driven multi-rule engine with glob scope). The proposal absorbed several of these into `ForbiddenTokens` without verification.

## The revised primitive set

After verification, the corpus collapses into ten reusable primitives plus six structurally bespoke lints. The set:

### Reusable primitives

1. **TokenScan**. Line-scan with optional comment-strip, string-strip, doc-comment-strip, word-boundary check, crate-scope filter, inline-allow check, proc-macro-skip. Position-agnostic (any line). Covers Pool A's `no_todo`, plus most Pool B (`NoAlloc`, `NoStd`, `NoBareNumeric`, `NoBareString`, `NoBareOption`, `NoBareResult`, `ArvoTypesOnly`, `NoDynDispatch`, `NoRuntimeSpawn`, `NoRuntimeRegistration`, `LintAllowRequiresTaskId`), plus Pool C's true duplicates and merges (~5-8 lints).

2. **AstNodePositionMatch**. Tree-sitter walk over specific node kinds (`macro_invocation`, `enum_item`, `impl_item`, `call_expression`, `field_expression`). Matches name or attribute against config list. Covers `no_self_define`, `no_pool_access`, `no_adhoc_error_enum`, `no_todo` (if we want AST not line-scan), `actionable_errors`, `no-vec-in-trait-sig`, `no-bare-static-str`. ~7 Pool A lints + 2 Pool B lints.

3. **AstTypePosition**. Tree-sitter walk over type-bearing positions (`struct_item.field_declaration`, `function_item.parameter_type`, `function_item.return_type`, `type_item.body`). Matches forbidden type list with optional visibility filter, category-based exemption, suppression-aware escalation. Covers Pool A's `no_bare_string`, `no_bare_result`, `no_bare_pub`, `no_bare_macro_types`, `no_box`, `no_float`, `no_primitive_key`, `no_raw_error_outside_primitives`, `no_vec_in_resource`, `repr_c_abi_safety`. Covers Pool B's `NoPublicRawField`, `StrategyMarkerRequired`, `TraitFirstSignatures`, `SemanticAliasNudge`. ~10 Pool A lints + 4 Pool B lints.

4. **IdentifierPattern**. Tree-sitter walk over named items; check name against suffix / prefix / regex lists; filter by item kind. Covers Pool A's `no_entry_suffix`. (Originally claimed to cover `no_manual_id`, but verification shows that lint is heuristic newtype detection not pure name matching; moves to BESPOKE.)

5. **ContentRegex**. Regex match against doc templates and rustdoc; multiple patterns per lint with ratio thresholds and code-fence stripping. Covers Pool B's `WritingStyle` only. (Originally claimed to cover `vocabulary-discipline`; verification shows that lint needs a dead-term-to-replacement table, see `TermReplacementTable` below.)

6. **TermReplacementTable** (NEW). Maps dead terms to canonical replacements; emits findings with per-term suggestion text. Word-boundary aware. Covers Pool C's `vocabulary_discipline`. May absorb future content lints with similar shape.

7. **FileMetric**. Per-file numeric metric plus threshold. Configurable metric (non-blank-non-comment lines, pub item count). Covers Pool A's `file_size`, `export_count`, `no_empty_crate`.

8. **UndocumentedItem**. Tree-sitter walk over pub items; check for rustdoc comment; optional escape via SHAME with min-words. Covers Pool A's `undocumented_type` (with cross-doc variant in the bespoke `design_doc_source_mismatch`).

9. **CrossDocSymbol**. Two-pass: collect symbols across all documents in pass 1, validate per-document in pass 2 with a configurable predicate. Covers Pool A's `no_duplicate_fn`, `single_source`. Partially covers `design_doc_source_mismatch` (cross-doc reference) and `deprecation_comparison` (cross-CL reference), but those need workflow-aware predicates that push them toward BESPOKE.

10. **WorkflowState**. Reads `mock/design_rounds/` and validates against the typestate layer. Covers Pool A's `changelist_doc_gate`, `changelist_immutability`, `changelist_lock`, `changelist_required`. Five lints (counting the existing `single_source` cross-doc plus four changelist).

### Bespoke primitives (cannot consolidate cleanly)

11. **SuppressionMeta**. Reads engine SuppressionMap, validates `tracked: #N` plus optional `reason`. Pool B's `LintAllowRequiresTaskId` is the v1 form; after SuppressionMap lands the v1 comment parser moves to read the engine map instead. Bespoke because it sits outside the lint dispatch model (meta-lint over engine state). One lint today, room for `overuse-of-allow` / `expired-tracked-allow`.

12. **`no_bare_vec` (Pool A)**. Two-phase mechanism: AST walk over type identifiers in Phase 1, text scan inside `define_*!` macro bodies in Phase 2. Dual exclusion logic (different in each phase). Cannot collapse into `AstTypePosition` without primitive carrying macro-body scanning, which is a separate concern.

13. **`no_manual_id` (Pool A)**. Heuristic newtype detection (`struct X(Y);` or `type X = Y;` patterns suggesting a manual ID wrapper). Pattern-shape matching on AST that does not generalise into name matching or type matching.

14. **`no_manual_impl` (Pool A)**. Heuristic detection of boilerplate impls (Clone, Copy, Debug, Default written by hand instead of derived). Pattern-recognition that does not fit any of the primitives.

15. **`no_adhoc_framework` (Pool A)**. Call-graph and structural pattern heuristic (dispatch tables, callback chains, init/run/cleanup triples). Genuinely irregular logic; would require a `CallGraphHeuristic` primitive that has only one user.

16. **`registrable_completeness` (Pool A)**. Validates that types implementing the `Registrable` trait provide all required methods. Trait-specific contract checking. Bespoke or absorb into a future `TraitContract` primitive once a second consumer emerges.

17. **`deprecation_comparison` (Pool A)**. Compares symbol presence between active and deprecated CLs across `mock/design_rounds/`. Workflow-aware cross-CL state. Could be absorbed into `WorkflowState` or kept as a separate `WorkflowSymbolDiff` primitive depending on how the workflow primitive set evolves.

18. **`design_doc_source_mismatch` (Pool A)**. Cross-doc reference check plus SHAME-entry escape semantics. Similar to `CrossDocSymbol` but with the workflow-state ingredient. Borderline; could absorb into `CrossDocSymbol` with a `predicate = "must-appear-in-design-doc"` config, plus a side hook to read SHAME entries. Schema memo decides.

19. **`forbidden_imports` (Pool A, soft-BESPOKE)**. Data-driven multi-rule engine with glob scope binding. Each rule has scope, forbidden list, reason, enabled flag. The proposal's `TokenScan` plus per-instance config could express each rule, but the multi-rule data shape is unique. Either ships as multiple `TokenScan` instances (one per rule) plus a config-load helper that expands the `[rule.*]` namespaced config, or stays as one bespoke multi-rule primitive.

The bespoke bucket is honest: these are lints whose mechanism does not generalise to other lints in the corpus. Forcing them into a primitive would mean the primitive grows config knobs only one lint uses, which is parametric leakage. Better to ship them as one-off `Lint` impls and keep the primitive set clean.

## Cross-pool duplicate verification

The migration plan at `mock/research/202605201500_lint-catalog-migration-plan.md` claimed eight per-repo lints could be deleted as byte-for-byte duplicates of stack-lints equivalents. Pool C verification falsifies the claim; the actual numbers:

### True duplicates (safe drop): 5 lints

- **`arvo/no_std_enforcer`** vs `NoStd`. Identical token list (`use std::`, `use ::std::`, `pub use std::`, `pub use ::std::`, `extern crate std`). Same severity, same exclusion patterns. Safe drop.
- **`arvo/no_dynamic_dispatch`** vs `NoDynDispatch`. Identical mechanism: token-boundary checking on `dyn `, TypeId, std::any / core::any. Same severity. Safe drop.
- **`hilavitkutin/no_dynamic_dispatch`** vs `NoDynDispatch`. Hilavitkutin version detects more dyn boundary forms (`*const dyn`, `*mut dyn` added). Functionally identical but more complete. Drop with verification that stack-lints includes the additional forms.
- **`hilavitkutin/no_runtime_spawn`** vs `NoRuntimeSpawn`. Identical forbidden list (`thread::spawn`, `tokio::spawn`, `rayon::spawn`, etc.). Crate-scoped to `hilavitkutin*` in arvo version. Safe drop after confirming stack-lints handles the scope filter (or just relying on per-repo config).
- **`hilavitkutin/no_runtime_registration`** vs `NoRuntimeRegistration`. Identical forbidden list (`inventory::submit!`, `#[ctor]`, `linkme`, etc.). Safe drop.

### Merges with care (drop after manual review): 3 lints

- **`arvo/no_alloc_enforcer`** vs `NoAlloc`. **Different token lists**. Arvo bans `Vec<`, ` String`, `Box<` only (3 tokens plus the `alloc::` import patterns). Stack-lints `NoAlloc` includes `Rc<`, `Arc<`, `vec!`, `String::new` and more. Migration must consciously decide: does v2's `no-alloc` ship arvo's narrower scope (more lenient) or stack-lints' broader scope (more strict)? Default: stack-lints' broader scope, since arvo missing tokens looks like a v1 incomplete implementation, not a deliberate scoping decision.
- **`hilavitkutin/no_std_enforcer`** vs `NoStd`. Token list matches. Hilavitkutin version adds an explicit crate-name filter (`hilavitkutin*`). This filter is the load-bearing difference: without it, the lint fires on arvo source consumed via hilavitkutin's dep tree. v2 migration: this becomes a `scope.crates` TOML field on the lint instance, not Rust code. Drop the per-repo file, add the scope to the consumer-side `lints.toml`.
- **`hilavitkutin/no_alloc_enforcer`** vs `NoAlloc`. Token list is **more complete** than arvo's (includes `Rc<`, `Arc<`). Has an interesting inline `lint:allow(no-alloc)` per-line suppression not present in stack-lints. The suppression mechanism is now SuppressionMap-driven in v2, so the inline check becomes obsolete. Drop after confirming SuppressionMap honours the same form.

### Keep as genuinely repo-specific: 4 lints

- **`arvo/strategy_marker_required`**. Walks specific arvo numeric crates (NUMERIC_CRATES list at line 17). Fires on missing `S: Strategy` parameter in `UFixed<>`, `IFixed<>`, etc. Severity is PUSH_GATE not HARD_ERROR. Carries an arvo-specific `// strategy-exempt:` comment-based escape. **No stack-lints equivalent**: stack-lints has its own `StrategyMarkerRequired` walking syn AST, but the arvo version has tighter crate scoping and the comment-based escape. The arvo version IS the better implementation for the arvo case; stack-lints version covers the general case for other repos.
- **`arvo/arvo_bits_traits_only`**. Crate-scoped to `arvo-bits` only. Walks struct items, enforces an `ALLOWED_OPAQUE_BITS` allowlist (currently just `Bits`). Marker-struct-tolerance (field-less structs allowed). **This is a layering rule specific to arvo-bits**, not generalisable.
- **`arvo/no_runtime_grow`**. Method-name scan for `.push(`, `.resize(`, `.extend(`, `Vec::with_capacity`. Severity ADVISORY, not HARD_ERROR. Signal-shaped lint for code review, not a blocker. Closely related to `no-alloc` but distinct: `no-alloc` bans the type, `no-runtime-grow` warns on usage patterns indicating growth intent. Arvo-shaped policy.
- **`hilavitkutin/vocabulary_discipline`**. The critical finding. Carries a dead-term-to-replacement table:
  ```
  chain → fiber
  chain_group → trunk
  partition → phase
  archetype → fiber
  entity → record
  Entity → record
  ```
  Per-term, the message includes the specific remedy. `ContentRegex` as proposed cannot express this because regex-plus-message has no parameter capture to suggestion-text. Either the new `TermReplacementTable` primitive ships (above) or this lint stays bespoke.

### Migration count

- **Safe drops**: 5
- **Merges with care**: 3 (deletion safe after confirming the broader stack-lints scope ships and SuppressionMap honours per-line allow)
- **Keep repo-specific**: 4

Compared to the migration plan's "drop 8 duplicates" claim: half the supposed duplicates have meaningful differences that the migration would have lost.

## Per-pool catalogs

Three subagent passes produced per-lint entries with file:line citations, AST node-kind lists, exclusion patterns, severity escalation rules, and proposed primitive assignments. The catalogs are extensive (Pool A alone has 37 lints across 18 mechanism categories). Rather than duplicate them here, the canonical pool catalogs live as appendices below.

### Pool A: mockspace built-ins (37 lints)

Source: `mockspace/lint-rules/src/`. Mechanism distribution:

| Mechanism | Count | Notes |
|---|---|---|
| `tree-sitter-walk` | 18 | Bulk of the AST-driven lints |
| `line-scan` | 3 | `file_size`, `no_empty_crate`, plus one other |
| `regex-content` | 1 | `forbidden_imports` (multi-rule data engine) |
| `multi-pass` | 7 | Cross-doc and two-phase lints |
| `WorkflowState` | 4 | `changelist_*` family |
| BESPOKE | 7 | Pattern-recognition and workflow-coupled lints |

Primitive assignments:

| Primitive | Count | Lints (selected) |
|---|---|---|
| `AstNodePositionMatch` | 10 | `actionable_errors`, `no_adhoc_error_enum`, `no_pool_access`, `no_self_define`, `no_todo` (if AST not line-scan), plus 5 more |
| `AstTypePosition` | 11 | `no_bare_string`, `no_bare_result`, `no_bare_pub`, `no_bare_macro_types`, `no_box`, `no_float`, `no_primitive_key`, `no_raw_error_outside_primitives`, `no_vec_in_resource`, `repr_c_abi_safety`, plus one more |
| `FileMetric` | 3 | `file_size`, `export_count`, `no_empty_crate` |
| `CrossDocSymbol` | 3 | `no_duplicate_fn`, `single_source`, partially `design_doc_source_mismatch` |
| `IdentifierPattern` | 1 | `no_entry_suffix` |
| `ContentRegex` | 1 | (writing-style; actually in Pool B not A) |
| `UndocumentedItem` | 2 | `undocumented_type` (with cross-doc variant), one more |
| `WorkflowState` | 4 | `changelist_doc_gate`, `changelist_immutability`, `changelist_lock`, `changelist_required` |
| BESPOKE | 7 | `deprecation_comparison`, `no_adhoc_framework`, `no_bare_vec`, `no_manual_id`, `no_manual_impl`, `registrable_completeness`, `forbidden_imports` (soft) |

Full per-lint detail with file:line citations preserved in the Pool A subagent output (committed alongside this note as `appendix-pool-a.md`, if we choose to ship the per-lint detail as a separate artifact; otherwise the audit transcript carries it).

### Pool B: stack-lints (18 lints)

Source: `mockspace-hilavitkutin-stack-lints/src/lints/`. Mechanism distribution:

| Mechanism | Count | Notes |
|---|---|---|
| `line-scan` | 9 | Token-list lints with strip + boundary checking |
| `tree-sitter-walk` | 8 | Type-position and AST-driven lints |
| `multi-pass` | 1 | `WritingStyle` (CrossCrateLint, file-system walk) |

Primitive assignments:

| Primitive | Count | Lints |
|---|---|---|
| `TokenScan` | 11 | `NoAlloc`, `NoStd`, `NoBareNumeric`, `NoBareString`, `NoBareOption`, `NoBareResult`, `ArvoTypesOnly`, `NoDynDispatch`, `NoRuntimeSpawn`, `NoRuntimeRegistration`, `LintAllowRequiresTaskId` |
| `AstTypePosition` | 5 | `NoPublicRawField`, `StrategyMarkerRequired`, `TraitFirstSignatures`, `SemanticAliasNudge`, plus one more |
| `AstNodePositionMatch` | 2 | `NoBareStaticStr`, `NoVecInTraitSig` |
| `ContentRegex` | 1 | `WritingStyle` |
| BESPOKE | 0 | All Pool B lints fit primitives cleanly |

Key Pool B findings:

- **`NoBareOption` lacks the prefix-filter that `NoBareResult` has.** Result lint skips `fmt::Result`, `io::Result`, `std::fmt::Result`, `std::io::Result` (lines 77-80, 92-105 of `no_bare_result.rs`). Option lint has no such filter despite doc claiming module-prefix exclusion (`no_bare_option.rs:67-69`). Either the Option lint is missing the filter (bug) or the doc is wrong. v2 migration: pick one, document, fix.
- **`ArvoTypesOnly`** is intended as the long-term canonical form; `NoBareNumeric` is retained for "config compatibility" (per doc lines 10-11). Migration plan was correct to drop `ArvoTypesOnly`; v2 keeps `NoBareNumeric` as the catalog name.
- **`WritingStyle`** is a CrossCrateLint that walks the workspace file system looking for `*.md.tmpl` patterns plus rustdoc comments from lib.rs. Multi-heuristic firing (em-dash density, word counts, exclamation counts, leading-list heuristic, label-colon-bullet heuristic). All severities PUSH_GATE. The `ContentRegex` primitive must support multi-pattern config plus threshold ratios to cover this.

### Pool C: per-repo custom lints (12 lints)

Source: `arvo/mock/lints/`, `hilavitkutin/mock/lints/`. (`vehje/mock/lints/` and `notko/mock/lints/` empty per Pool C verification.) Mechanism distribution:

| Mechanism | Count | Notes |
|---|---|---|
| `line-scan` | 12 | All Pool C lints are line-scan with crate-scope filters |
| `tree-sitter-walk` | 0 | No AST-based per-repo lints |

Primitive assignments:

| Primitive | Count | Lints |
|---|---|---|
| `TokenScan` | 6 | True duplicates + merges (drop 5, merge 3 = 8 of these become catalog instances) |
| `IdentifierPattern` or BESPOKE | 1 | `arvo_bits_traits_only` (allowlist-driven, repo-specific) |
| `TermReplacementTable` (new) | 1 | `vocabulary_discipline` |
| BESPOKE | 4 | `strategy_marker_required` (arvo), `arvo_bits_traits_only`, `no_runtime_grow`, `vocabulary_discipline` |

Pool C is overwhelmingly `TokenScan`-shaped with crate-scope filters. The interesting cases are the four repo-specific keepers (above) and the `TermReplacementTable` finding for `vocabulary_discipline`.

## Implications for the revised proposal

The proposal at `202605202200_lint-primitive-consolidation.md` needs the following corrections:

### Primitive count and naming

- **Old claim**: 7 reusable + 3 bespoke.
- **Verified count**: 10 reusable + 6 bespoke + 1 new primitive type (`TermReplacementTable`) needed for `vocabulary_discipline`.

The proposal's `ForbiddenTokens` was a fiction: the audit showed it conflated two distinct mechanisms (line-scan TokenScan with no AST awareness, and tree-sitter AstNodePositionMatch with node-kind-driven walk). Split into `TokenScan` and `AstNodePositionMatch` per reviewer item #3. Additionally separate `AstTypePosition` since type-position checks have type-string-match semantics that differ from name-match.

### Bespoke bucket

- **Old claim**: 3 bespoke (`CrossDocSymbolCheck`, `WorkflowStateValidator`, `SuppressionMeta`).
- **Verified bespoke**: 6 (the above plus `no_bare_vec`, `no_manual_id`, `no_manual_impl`, `no_adhoc_framework`, `registrable_completeness`, `forbidden_imports` if not folded into multi-TokenScan).

The proposal underestimated the BESPOKE bucket because the shallow audit assumed AST-shape-similarity equated to mechanism-similarity. The line-level verification showed that pattern-recognition lints (`no_manual_id`, `no_manual_impl`, `no_adhoc_framework`) each carry unique heuristic logic that no generic primitive expresses.

### Migration deletion count

- **Old claim**: drop 8 byte-for-byte duplicates.
- **Verified count**: drop 5 safe duplicates, merge 3 with care (verify token-list completeness and SuppressionMap honours per-line allow before deletion), keep 4 as genuinely repo-specific.

### The vocabulary_discipline finding

The proposal's claim that `ContentRegex` covers `vocabulary-discipline` is false. The lint carries a structured dead-term-to-replacement table; the message per finding is per-term, not a fixed template. `ContentRegex` as defined (regex + message + ratio threshold) has no capture-to-suggestion mapping.

Two options for the revised proposal:

- **Option A**: Add `TermReplacementTable` as an eleventh reusable primitive. Config form: `replacements: { "chain" = "fiber", "partition" = "phase", ... }` with word-boundary matching and optional crate-scope filter. The primitive covers `vocabulary_discipline` plus any future content lints with similar shape.
- **Option B**: Keep `vocabulary_discipline` as the seventh bespoke lint.

Recommendation: Option A. The shape is reusable enough that future content lints will want it (e.g., a "deprecated terminology in docs" lint that maps `substrate → foundations`, `HList → cons-list` workspace-wide). One primitive, used by one lint today, room for more.

### Other corrections from the reviewer findings

The reviewer's other P0 items still stand and apply to the revised proposal:

- **#1 (AST cache trait location)**: still unresolved. Pick now: either `MockspaceDocument` ships AST methods on the concrete type (and engine works generically over `E::Document` not `&dyn Document`), or mockspace-core's `Document` trait grows methods (breaks the lock claim, forces `syn` into the foundations crate). The catalog assignments do not change which path is right; the schema memo decides.
- **#2 (PerDocument wrapper cost)**: still unresolved. Either restore `check_document` method on the trait (two methods, one trait, engine picks based on `mode()`), or accept the 6000-wrapper-per-run cost. The catalog assignments do not pressure this choice; design-driven.
- **#4 (push-gate detached HEAD)**: still unresolved.
- **#5 (duplicate count)**: corrected in this audit. Revised proposal carries the corrected count.

## What this audit unblocks

1. **Proposal revision (task #518)**: now has the verified primitive count, the corrected duplicate count, and the explicit BESPOKE bucket. Revision pass updates the proposal text inline.
2. **Schema design memo**: now has honest primitive count to lock per-primitive config schemas against. Schema memo cannot lock anything until the primitive count is settled; this audit settles it.
3. **Phase 2D implementation order**: ten primitives plus six bespoke is the correct file count for `mockspace-rs/src/builtins/`. Estimate: 11 primitive files (one per reusable plus the catalog) plus six bespoke files plus a catalog registry. Compared to the proposal's "10 primitive files" claim, the corrected number is closer to "17 files" (10 reusable + 6 bespoke + 1 catalog).

## References

- **Pool A subagent transcript** (Pool A audit): 37 per-lint entries with file:line citations covering `mockspace/lint-rules/src/*.rs`. Mechanism counts, primitive assignments, 7 BESPOKE flags with reasons.
- **Pool B subagent transcript** (Pool B audit): 18 per-lint entries covering `mockspace-hilavitkutin-stack-lints/src/lints/*.rs`. Token lists verified, `NoBareOption`-vs-`NoBareResult` prefix-filter discrepancy flagged.
- **Pool C subagent transcript** (Pool C audit): 12 per-lint entries covering `arvo/mock/lints/`, `hilavitkutin/mock/lints/`. All 9 claimed duplicates verified line-by-line; `vocabulary_discipline` dead-term-to-replacement table verified at file:line.
- Proposal: `mock/research/202605202200_lint-primitive-consolidation.md` (corrections inline during revision).
- Reviewer findings: `mock/research/202605202300_lint-primitive-proposal-review.md` (items #3 and #5 verified by this audit).
- Migration plan: `mock/research/202605201500_lint-catalog-migration-plan.md` (deletion count revision lands during proposal revision).

## Recorded

2026-05-21 timestamp. Audit pass replaces the shallow per-name inference that the original consolidation proposal leaned on. Three parallel subagent reads, each ~600-2500 words of per-lint detail, line-level citations for every claim. The audit's correction count: primitive set 7→10 reusable, bespoke bucket 3→6, duplicate drop 8→5 (plus 3 merges and 4 keepers).

The discipline this audit exercises is the one workspace memory `feedback_verify_subagent_claims` warns about: subagents can produce confident structural claims that fall apart under line-level verification. Three parallel deep reads with file:line citations is the cost of getting the corpus inventory right. The cost is one-time; the input it produces is the lock for the schema design memo.
