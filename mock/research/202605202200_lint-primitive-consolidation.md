# Lint primitive set, scoping model, and v1 cleanup

**Date:** 2026-05-20 (revised 2026-05-21 per reviewer findings + corpus audit)
**Status:** Proposal, pre-implementation. Revision 2 incorporates the senior review at `mock/research/202605202300_lint-primitive-proposal-review.md` and the per-lint mechanism audit at `mock/research/202605210000_lint-corpus-mechanism-audit.md`.
**Scope:** Mockspace v2 Phase 2D revision. Replaces the "port 63 lints individually" plan with a "build 10 reusable primitives + 1 new replacement-table primitive + 6 bespoke" plan, expressed as TOML in `lints.toml`. Also resolves the scoping and operational pain points the v1 lint runtime carries.
**Sibling notes:**
- `mock/research/202605201400_viola-engine-integration-shape.md`
- `mock/research/202605201500_lint-catalog-migration-plan.md`
- `mock/research/202605201700_engine-preprocessor-architecture.md`
- `mock/research/202605202300_lint-primitive-proposal-review.md` (senior review)
- `mock/research/202605210000_lint-corpus-mechanism-audit.md` (verified per-lint catalog)

## Revisions from v1

The v1 proposal claimed 7 reusable + 3 bespoke primitives plus 8 byte-for-byte duplicate drops. The senior review falsified both claims at file:line. The corpus audit walked all 63 lints independently and produced the verified counts. This revision carries those corrections inline. Concrete deltas:

1. Primitive count: 7 reusable + 3 bespoke becomes **11 reusable (incl. new `TermReplacementTable`) + 6 bespoke**. `ForbiddenTokens` splits into `TokenScan` + `AstNodePositionMatch` + `AstTypePosition` (three different AST mechanisms, cannot unify by config alone).
2. Duplicate count: 8 safe drops becomes **5 safe + 3 merges + 4 keepers**. The "byte-for-byte" claim was wrong; most claimed duplicates differ at token list, scope filter, or exclusion logic.
3. `ContentRegex` does not cover `vocabulary-discipline`. New `TermReplacementTable` primitive ships for dead-term-to-replacement tables.
4. AST cache location decided: concrete `MockspaceDocument` (foundation `Document` trait stays at 4 methods, no `syn` in mockspace-core).
5. `Lint` trait grows two methods (`check_document` for PerDocument mode, `check_project` for Project / TwoPhase modes); avoids per-document wrapper-`Project` allocation cost.
6. Push-gate diff base on detached HEAD: env override + `@{upstream}` fallback + full-project last resort.
7. CLI `--scope` semantics: intersect, not override. Empty intersect drops the lint silently with a verbose log.
8. Config errors: separate `Vec<ConfigError>` channel, not `Finding`.
9. `CatalogEntry::kind: &'static str`, not closed enum. Stack-lints contributes entries via static registration (e.g. `inventory` / `linkme`).
10. Editor surface bypasses staging entirely; runs on the currently-edited buffer document.

## Why this note exists

The Phase 2D plan as it stood ported 16 mockspace built-in lints to the new `Lint` / `PerDocumentLint` traits as 16 separate Rust files, plus separately reorganised the stack-lints pack into another 17 separate files, plus migrated 4 per-repo custom lints. The work is mechanical but the result preserves a structural problem: v1 ships ~63 lints implemented as parallel disjointed siblings, several of which do the same thing with slightly different code and slightly different bugs. Porting them individually carries the bugs forward.

The verified audit across the three pools (mockspace built-ins, stack-lints pack, per-repo custom lints) shows that most of the corpus collapses into a small set of configurable primitives. The same audit shows recurring v1 operational pain (cross-crate analysis as a parallel trait hierarchy, per-lint hardcoded crate scoping, no staging awareness, inconsistent boundary checking, per-lint TOML parsing) that consumer repos have learned to work around without anyone fixing the root causes.

This note proposes:

1. A primitive set (10 reusable + 1 replacement-table primitive + 6 bespoke) that subsumes the catalog.
2. A unified scoping model that replaces every per-lint hardcoded filter.
3. A cleanup of the v1 operational pain points by making each one a property of the engine or the primitive base rather than per-lint code.
4. A migration order that keeps consumer disruption to one PR cycle.

The user-facing framing remains "code can be naive, viola lands soon". The consolidation does not chase performance; it chases bug surface and migration surface. Eleven primitive impls plus six bespoke migrate to viola plugins later as seventeen plugins. Sixty-three bespoke lints would migrate as sixty-three plugins. The ratio is the point.

## Scope of the proposal

This note covers the engine-internal lint catalog of mockspace-rs (the Rust engine living at `mockspace/mock/crates/mockspace-rs/`) and the consumer-facing `lints.toml` configuration. It does not change the foundation crate (already settled in `mock/research/202605201700_engine-preprocessor-architecture.md`). It does not change the workflow side (manifests, phases, typestate). It does not commit to a viola plugin SDK shape; that work is downstream and the primitive set is the input.

The migration plan still applies in shape; the counts revise: 16 lints stay built-in to mockspace-rs as catalog entries, 17 move to stack-lints, 5 drop as safe duplicates + 3 merge after token-list reconciliation, 4 stay repo-local. What changes here is how those four pools are *implemented*: instead of one file per lint, eleven files for the reusable primitives plus six for the bespoke ones plus per-primitive catalog entries that name the lint and provide its config.

## Pain points to resolve

The audit surfaced these recurring problems in v1 lint code:

1. **Boundary checking reinvented per lint.** Some lints check word boundaries before matching, some do not. False positives on identifier names containing the forbidden token (`MyVec` matching `Vec`, `usize_to_dec` matching `usize`) appear in some lints and not others. There is no shared utility.
2. **String and comment stripping inconsistent.** Some lints strip string literals and comments before scanning, some do not. Findings inside rustdoc example code are a routine false positive. Each lint that bothers has its own stripper.
3. **AST re-parsed per lint per file.** Lints that need tree-sitter parse fresh on entry. For fifty lints over two hundred files, that is ten thousand parses. Engine-level caching does not exist.
4. **Per-lint hardcoded crate scoping.** `arvo_bits_traits_only` checks `crate_name == "arvo-bits"` inline. `vocabulary_discipline` checks `crate_name.starts_with("hilavitkutin")` inline. The scoping rule is policy, not implementation, but it sits in lint code.
5. **`primitive-introductions` exemption hardcoded.** Each lint that wants to exempt a category calls `ctx.introduces(primitive)` manually. The mechanism exists; every lint has to know to use it.
6. **`lint:allow(...)` parsed per lint.** Each lint reads source for its own escape-hatch comments. Parsers vary in robustness. Some honour `tracked: #N`, some do not. The engine now has SuppressionMap centralising this, but the legacy lints have not been updated.
7. **Two parallel trait hierarchies.** `Lint` and `CrossCrateLint` cover per-file and project-wide patterns separately. Consumers have to know which to implement. The signatures differ, the dispatch path differs, the lint registration call differs.
8. **No file-glob exemption.** Per-lint exemption is by crate-name suffix. Cannot say "exempt all FFI source under `src/ffi/**`" without writing custom logic.
9. **No staging awareness.** Lints run on every file every time. Pre-commit hook does not consult `git diff --name-only --cached`. Big repos pay the full cost on every commit.
10. **`--scope <crate>` flag bolted on top.** Layered over the per-lint hardcoded scoping rather than being the unified mechanism.
11. **Config errors panic mid-run.** `configure(HashMap<String, String>)` parses TOML values lazily inside lint dispatch. Bad config crashes during scan, not at load.
12. **Lint output is just message strings.** No `rule_id` linking back to documentation. Consumers cannot easily look up "what does this lint want me to do".
13. **`--fix` machinery absent.** `FixSuggestion` exists on Finding but no command applies it. Suggestions go unused.
14. **Severity downgrade requires Rust edit.** Some lints hardcode severities inline. Tuning gates means a recompile, not a TOML edit.
15. **Per-repo lints and stack-lints overlap inconsistently.** The audit verified the actual overlap is 5 safe drops + 3 careful merges + 4 genuine keepers. Maintaining the overlap without coordination means fixing bugs twice for the safe cases and missing important scope differences for the merge cases.

Items 1 through 8 fix at the primitive layer. Items 9 and 10 fix at the engine layer via the scoping model. Item 11 fixes at config load time. Items 12 through 14 fix at the catalog and CLI layer. Item 15 fixes via the per-duplicate decision recorded in the audit.

## The primitive set

Ten reusable primitives plus one new replacement-table primitive cover roughly fifty-seven lints by varying configuration. Six primitives stay bespoke because their mechanism is genuinely irregular and forcing them into a primitive would mean the primitive grows config knobs only one lint uses (parametric leakage).

### Reusable primitives

**1. TokenScan.** Line-based source-text scan for tokens. Optional word-boundary check, optional string-strip, optional comment-strip (including rustdoc), per-crate-scope filter, inline-allow check, proc-macro-skip. No AST. Covers `no_todo`, plus Pool B's `NoAlloc`, `NoStd`, `NoBareNumeric`, `NoBareString`, `NoBareOption`, `NoBareResult`, `ArvoTypesOnly`, `NoDynDispatch`, `NoRuntimeSpawn`, `NoRuntimeRegistration`, `LintAllowRequiresTaskId`, plus Pool C's safe duplicates and merges. About 12-14 lints.

**2. AstNodePositionMatch.** Tree-sitter walk over specific node kinds (`macro_invocation`, `enum_item`, `impl_item`, `call_expression`, `field_expression`). Matches name or attribute against config list. Covers `no_self_define`, `no_pool_access`, `no_adhoc_error_enum`, `actionable_errors`, plus Pool B's `NoBareStaticStr`, `NoVecInTraitSig`, plus 3-4 Pool A lints. About 9-10 lints.

**3. AstTypePosition.** Tree-sitter walk over type-bearing positions (`struct_item.field_declaration`, `function_item.parameter_type`, `function_item.return_type`, `type_item.body`). Matches forbidden type list with optional visibility filter, category-based exemption, suppression-aware escalation. Covers Pool A's `no_bare_string`, `no_bare_result`, `no_bare_pub`, `no_bare_macro_types`, `no_box`, `no_float`, `no_primitive_key`, `no_raw_error_outside_primitives`, `no_vec_in_resource`, `repr_c_abi_safety`, plus Pool B's `NoPublicRawField`, `StrategyMarkerRequired`, `TraitFirstSignatures`, `SemanticAliasNudge`. About 14-15 lints. (Absorbs the v1-proposal's separate `StructFieldShape` and `FnSignatureShape`; both are type-position checks differing only in node kind.)

**4. IdentifierPattern.** Tree-sitter walk over named items (struct, enum, fn, trait, type alias, const). Match the identifier against prefix, suffix, and regex lists. Filter by item kind and visibility. Covers Pool A's `no_entry_suffix`. (`no_manual_id` was originally claimed; verification showed that lint is heuristic newtype detection, not pure name matching. Moves to BESPOKE.)

**5. ContentRegex.** Regex match against `.md.tmpl`, `.md`, and rustdoc comments. Multiple patterns per lint, each with optional ratio threshold (one match per N lines tolerated, beyond that fires). Covers Pool B's `WritingStyle` (em-dashes, marketing words, filler, greeting openers). Does NOT cover `vocabulary-discipline`; that needs `TermReplacementTable` (see below).

**6. TermReplacementTable.** (New since v1.) Maps dead terms to canonical replacements; emits findings whose message is parametrised on the matched term plus the configured replacement. Word-boundary aware, optional crate-scope filter. Config form: `replacements = { "chain" = "fiber", "partition" = "phase", ... }`. Covers Pool C's `vocabulary_discipline` plus any future content lint with the same shape (e.g. workspace-wide "deprecated terminology" lints mapping `substrate -> foundations`, `HList -> cons-list`).

**7. FileMetric.** Per-file numeric metric (line count, non-blank-non-comment line count, pub item count, etc.) with a threshold. One finding per file that exceeds. Covers `file-size`, `export-count`, `no-empty-crate`. About three lints.

**8. UndocumentedItem.** AST walk over public items. Check for preceding rustdoc comment. Optional escape via SHAME entry (a documented exception in a per-crate SHAME.md.tmpl with at least N words of rationale). Covers `undocumented-type`. Room for `undocumented-fn`, `undocumented-trait`, `undocumented-enum-variant` as separate instances.

**9. CrossDocSymbolCheck.** Two-pass collect-then-validate. Pass 1 walks all documents collecting symbols of a configured kind (type names, fn names, manifest entries). Pass 2 walks per-document checking each defined symbol against the Pass 1 set with a configured predicate (`must-appear-in-design-doc`, `no-duplicates-across-crates`, `must-match-deprecation-entry`). Covers `no_duplicate_fn`, `single_source`, and partially `design_doc_source_mismatch` (cross-doc reference part; the SHAME-entry escape may move it to bespoke).

**10. WorkflowState.** Reads `mock/design_rounds/` and validates filename conventions, state-machine transitions, and lock semantics against the typestate layer in mockspace-core. Inherently mockspace-shaped because the rules are about the workflow itself. Covers `changelist_doc_gate`, `changelist_immutability`, `changelist_lock`, `changelist_required`. About four lints (room for `registrable_completeness` to consolidate here if a `TraitContract` extension lands).

**11. SuppressionMeta.** Reads the engine's SuppressionMap (populated by preprocessors) and validates constraints: every suppression must carry a `tracked = "#N"`, optional `reason = "..."` enforcement, optional expiry checking against task state. Covers Pool B's `LintAllowRequiresTaskId` (the v1 line-scan form retires once the engine map is the source of truth). Room for `overuse-of-allow` (per-crate count threshold), `expired-tracked-allow` (task is closed), `undocumented-allow` (no reason field).

### Bespoke primitives

The audit identified six lints whose mechanism does not generalise to others in the corpus. Forcing them into a generic primitive would mean the primitive grows config knobs only one lint uses, which is parametric leakage. Better to ship them as one-off `Lint` impls and keep the primitive set clean.

**12. `no_bare_vec` (Pool A).** Two-phase mechanism: AST walk over type identifiers in Phase 1, text scan inside `define_*!` macro bodies in Phase 2. Dual exclusion logic (different in each phase). Cannot collapse into `AstTypePosition` without primitive carrying macro-body scanning, which is a separate concern.

**13. `no_manual_id` (Pool A).** Heuristic newtype detection (`struct X(Y);` or `type X = Y;` patterns suggesting a manual ID wrapper). Pattern-shape matching on AST that does not generalise into name matching or type matching.

**14. `no_manual_impl` (Pool A).** Heuristic detection of boilerplate impls (Clone, Copy, Debug, Default written by hand instead of derived). Pattern-recognition that does not fit any of the primitives.

**15. `no_adhoc_framework` (Pool A).** Call-graph and structural pattern heuristic (dispatch tables, callback chains, init/run/cleanup triples). Genuinely irregular logic; would require a `CallGraphHeuristic` primitive that has only one user.

**16. `registrable_completeness` (Pool A).** Validates that types implementing the `Registrable` trait provide all required methods. Trait-specific contract checking. Bespoke or absorb into a future `TraitContract` primitive once a second consumer emerges.

**17. `deprecation_comparison` (Pool A).** Compares symbol presence between active and deprecated CLs across `mock/design_rounds/`. Workflow-aware cross-CL state. Borderline with `WorkflowState`; the schema memo decides whether it absorbs there or stays bespoke.

`forbidden_imports` is soft-BESPOKE: a data-driven multi-rule engine with glob scope binding. The audit notes it can ship as multiple `TokenScan` instances (one per rule) plus a config-load helper that expands the `[rule.*]` namespaced config. Schema memo locks the choice.

### What drops

The migration drops:

- **Five safe duplicates** (verified byte-for-byte equivalent or strictly subset-of stack-lints): `arvo/no_std_enforcer`, `arvo/no_dynamic_dispatch`, `hilavitkutin/no_dynamic_dispatch` (after verifying stack-lints includes the additional `*const dyn` / `*mut dyn` forms), `hilavitkutin/no_runtime_spawn`, `hilavitkutin/no_runtime_registration`.
- **Three merges with care** (drop after verification): `arvo/no_alloc_enforcer` (stack-lints has broader token list; default to broader), `hilavitkutin/no_std_enforcer` (carries an arvo-vs-hilavitkutin crate-scope filter that moves into `lints.toml` scope config), `hilavitkutin/no_alloc_enforcer` (has inline `lint:allow(no-alloc)` per-line suppression now obsolete under SuppressionMap).
- **Four genuine keepers** (do not drop; repo-specific mechanisms): `arvo/strategy_marker_required` (tighter crate scoping + comment-based escape), `arvo/arvo_bits_traits_only` (allowlist-driven layering rule), `arvo/no_runtime_grow` (advisory growth-pattern lint), `hilavitkutin/vocabulary_discipline` (covered by `TermReplacementTable` primitive but stays per-repo for the table content).
- **`arvo_types_only`** from stack-lints. Redundant with `no-bare-numeric`; migration plan calls it out.
- **`no_entry_suffix`** from mockspace built-ins. Clause-historical, no longer reflects active policy.

That is 63 lints minus 7 deletions (5 safe + 2 stack-lints redundancies), leaving ~56 entries. Roughly fifty instances of the eleven reusable primitives plus six bespoke. Seventeen Rust files in total: eleven for reusable primitives plus six for bespoke.

## Scoping as three orthogonal axes

Every primitive consumes the same scoping model. Lint code never inspects crate names or paths; the engine pre-filters the document slice each lint sees according to the configured scope.

### Path filter

Which Documents a lint sees. TOML form:

```toml
[lints.no-bare-vec.scope]
paths = ["**/*.rs"]
exempt_paths = ["**/ffi/**", "**/tests/**"]
crates = ["*"]
exempt_crates = ["arvo"]
languages = ["rust"]
exempt_categories = ["bare-collection"]
proc_macro_exempt = true
```

The engine applies the filter once per lint per gate. Lints declared with `paths = ["**/*.md.tmpl"]` only see markdown documents. The path glob library is `globset`; syntax (`**`, `*`, brace expansion, `!` negation) documented in the schema memo.

`exempt_categories` is the unified replacement for the per-lint `ctx.introduces(primitive)` call. The engine reads `[primitive-introductions]` once, builds a per-crate category set, and excludes documents whose crate has any of the listed categories. The lint never sees those documents.

`proc_macro_exempt = true` means the engine excludes documents in any crate listed in `proc_macro_crates` (a Cargo.toml-derived set). Same mechanism, different source of the exemption set.

**`visibility` is per-primitive config, not unified scope.** Primitives that consume visibility filtering (AstTypePosition, IdentifierPattern, UndocumentedItem) declare a `visibility` field inside their own `[lints.<name>.config]` block. The scope block has no visibility field; setting one is a config-validation error. This makes the dependency between filter and primitive mechanism explicit. (Was hidden coupling in v1: TokenScan silently ignored `scope.visibility`.)

### Mode

How the lint sees Documents. Each primitive declares its mode in the catalog, not in user config. The engine routes:

- **PerDocument**: engine iterates `project.documents()`, calls `check_document(ctx, doc, sink)` per file. Primitives 1, 2, 3, 4, 5, 6, 7, 8.
- **ProjectScoped**: engine calls `check_project(ctx, project, sink)` once. Primitive 11 (SuppressionMeta reads the engine map).
- **TwoPhaseProject**: engine calls `check_project(ctx, project, sink)` once and the primitive walks documents twice internally (collect, then validate). Primitives 9, 10.

The unified trait surface in mockspace-rs (concrete document, not foundation trait object):

```rust
pub trait Lint: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn default_severity(&self) -> GateSeverity;
    fn mode(&self) -> LintMode;

    /// PerDocument mode dispatches here once per Document.
    /// Default impl: unreachable (engine never calls it for non-PerDocument modes).
    fn check_document(
        &self,
        ctx: &LintContext<'_>,
        doc: &MockspaceDocument,
        sink: &mut dyn FindingSink,
    ) -> Result<(), LintError> {
        let _ = (ctx, doc, sink);
        unreachable!("check_document called on non-PerDocument lint {}", self.name());
    }

    /// ProjectScoped and TwoPhaseProject modes dispatch here once per run.
    /// Default impl: unreachable.
    fn check_project(
        &self,
        ctx: &LintContext<'_>,
        project: &MockspaceProject,
        sink: &mut dyn FindingSink,
    ) -> Result<(), LintError> {
        let _ = (ctx, project, sink);
        unreachable!("check_project called on PerDocument lint {}", self.name());
    }
}
```

The engine dispatches based on `mode()`. PerDocument lints implement `check_document` only; ProjectScoped and TwoPhase lints implement `check_project` only. The default `unreachable!()` is correct because `mode()` is the dispatch table and the engine never calls the other method.

**AST cache lives on concrete `MockspaceDocument`**, not on the foundation `Document` trait. Foundation crate stays untouched (4 trait methods: `path`, `language`, `source`, `content_hash`). The concrete type adds:

```rust
impl MockspaceDocument {
    pub fn ast(&self) -> Option<&syn::File> { /* cached, syn parse once */ }
    pub fn tree_sitter(&self) -> Option<&tree_sitter::Tree> { /* cached, ts parse once */ }
    pub fn source_stripped(&self, opts: StripOpts) -> &str { /* cached per opts */ }
}
```

Because the `Lint` trait takes `&MockspaceDocument` (not `&dyn Document`), AST access is direct. The catalog ships `Vec<Box<dyn Lint>>` with no trait-object-erasure problem because `Lint` is not generic over Document. `syn` does not enter mockspace-core.

**`MockspaceProject` carries the surface lints read through.** Several design decisions implicitly reach into the project beyond `documents()`; enumerating the surface now removes the open-question hand-wave:

```rust
impl MockspaceProject {
    /// All documents in the project, regardless of staging.
    pub fn documents(&self) -> impl Iterator<Item = &MockspaceDocument>;

    /// Documents filtered by the active StagingFilter for the current gate.
    /// Engine pre-filters before dispatch on staging_aware = true lints.
    pub fn staged_documents(&self) -> impl Iterator<Item = &MockspaceDocument>;

    /// Crate-to-document mapping; drives scope.crates filtering and proc_macro_exempt.
    pub fn crate_graph(&self) -> &CrateGraph;

    /// Workspace-wide metadata (workspace root, member crates, Cargo metadata).
    pub fn workspace(&self) -> &WorkspaceMetadata;

    /// Access to mock/design_rounds/ state; used by WorkflowState primitive.
    pub fn design_rounds(&self) -> &DesignRoundsView;

    /// Engine-internal SuppressionMap; used by SuppressionMeta primitive.
    pub fn suppressions(&self) -> &SuppressionMap;

    /// Active run surface (Local / Ci / Editor) and gate (Commit / Build / Push).
    pub fn surface(&self) -> RunSurface;
    pub fn gate(&self) -> Gate;

    /// Categories declared in [primitive-introductions] per crate.
    /// Drives scope.exempt_categories pre-filter.
    pub fn introduced_categories(&self, crate_name: &str) -> &CategorySet;
}
```

PerDocument lints typically read `documents()` via the engine's filtered slice and never touch the rest. TwoPhase lints (`CrossDocSymbolCheck`, `WorkflowState`) iterate `documents()` in Pass 1 and either `documents()` or `staged_documents()` in Pass 2 per `staging_aware`. `WorkflowState` reaches `design_rounds()`. `SuppressionMeta` reaches `suppressions()`. Scope pre-filtering on `crate.crates` / `proc_macro_exempt` / `exempt_categories` runs in the engine using `crate_graph()` and `introduced_categories()`; lints never inspect those surfaces themselves.

`CrateGraph`, `WorkspaceMetadata`, `DesignRoundsView`, `CategorySet` are mockspace-rs-internal concrete types. The schema memo locks their shape; this enumeration commits the surface members lints depend on.

### Gate scope and staging

When the lint runs and what subset it sees. Per-gate TOML form:

```toml
[lints.no-bare-vec.gate.commit]
severity = "warn"
only_staged = true

[lints.no-bare-vec.gate.build]
severity = "error"
only_staged = false

[lints.no-bare-vec.gate.push]
severity = "error"
only_staged = false
skip = false
```

The engine constructs a `StagingFilter` per run. Per gate:

- **commit**: `git diff --name-only --cached`.
- **push**: see "Push-gate diff base" below for detached-HEAD handling.
- **build**: no staging filter; full project.

When a lint declares `only_staged = true` at gate G, the engine filters the lint's Document slice to staged paths before dispatch.

**`staging_aware` is a single flag with mode-dependent meaning**, not a per-phase split. The field on `CatalogEntry` means "the lint's per-document phase honors `StagingFilter` when `only_staged = true`":

- **PerDocument mode**: `staging_aware = true` means the only pass honors the staging filter. `staging_aware = false` means the lint always sees the full document set regardless of `only_staged`.
- **TwoPhaseProject mode**: `staging_aware = true` means Pass 1 (collection) walks the full project unconditionally, Pass 2 (per-document validation) honors the staging filter when `only_staged = true`. Pass 1 cannot be staging-filtered because cross-doc correctness requires the full symbol table. `staging_aware = false` means both passes always see the full document set.
- **ProjectScoped mode**: `staging_aware = false` is the only legal value. The lint reads engine-internal state (e.g., SuppressionMap) that has no per-document interpretation; config validation rejects `only_staged = true` with a clear error.

Concrete catalog values:

| Primitive | Mode | staging_aware |
|---|---|---|
| TokenScan, AstNodePositionMatch, AstTypePosition, IdentifierPattern, ContentRegex, TermReplacementTable, FileMetric, UndocumentedItem | PerDocument | `true` |
| CrossDocSymbolCheck | TwoPhaseProject | `true` (Pass 2 honors staging; Pass 1 walks all) |
| WorkflowState | TwoPhaseProject | `false` (design-round consistency requires the full tree on both passes) |
| SuppressionMeta | ProjectScoped | `false` |

This preserves cross-doc correctness (Pass 1 always sees everything) while letting consumers gate "validate only the staged subset" workflows on TwoPhase lints whose validation is per-document. Config validation rule: `only_staged = true` is accepted iff `staging_aware = true`; the per-mode semantics above are documentation, not separate validation rules.

`skip = true` disables the lint at that gate regardless of severity. Useful for "writing-style only at push" or "file-size never at commit".

#### Push-gate diff base

On a normal feature branch with an upstream, `git rev-parse @{upstream}` resolves and the base is `git merge-base HEAD @{upstream}`. Resolution order:

1. If env var `MOCKSPACE_PUSH_DIFF_BASE` is set, use its value as the rev. CI sets this explicitly (typical: `origin/dev`).
2. Else if `git rev-parse @{upstream}` succeeds, use `git merge-base HEAD @{upstream}`.
3. Else (detached HEAD with no env override, no upstream): full-project scan with a one-line warning logged to stderr. Lint runs as if `only_staged = false` regardless of TOML.

CI workflows that check out a commit (detached HEAD) set `MOCKSPACE_PUSH_DIFF_BASE` to the appropriate base ref. Local push from a tracked branch needs no env var.

### Editor surface (LSP integration)

`RunSurface::Editor` bypasses the staging filter entirely. The editor passes the currently-edited buffer to the engine; the engine runs lints over that one document only, using `commit` gate severities. Staging-aware logic is irrelevant because the document being edited may not be staged (and probably is not, mid-edit). The schema memo locks this and the `LintContext` carries a `surface()` accessor that primitives can read if they want to adjust behaviour (most will not).

## Cross-crate as one trait

V1's `Lint` plus `CrossCrateLint` split disappears. One trait, three modes (above). Authoring a new project-scoped lint becomes:

```rust
impl Lint for NoDuplicateFn {
    fn name(&self) -> &'static str { "no-duplicate-fn" }
    fn description(&self) -> &'static str { "..." }
    fn default_severity(&self) -> GateSeverity { GateSeverity::uniform(Severity::Error) }
    fn mode(&self) -> LintMode { LintMode::TwoPhaseProject }

    fn check_project(
        &self,
        ctx: &LintContext<'_>,
        project: &MockspaceProject,
        sink: &mut dyn FindingSink,
    ) -> Result<(), LintError> {
        let mut symbols: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for doc in project.documents() {
            if let Some(ast) = doc.ast() {
                for fn_name in extract_pub_fns(ast) {
                    symbols.entry(fn_name).or_default().push(doc.path().to_owned());
                }
            }
        }
        for (name, sites) in symbols {
            if sites.len() > 1 {
                sink.emit(/* finding with all sites */);
            }
        }
        Ok(())
    }
}
```

No marker trait, no separate cross-crate dispatch path. The engine looks at `mode()`, calls `check_project` once with the full project, and the lint walks documents on its own. `CrossDocSymbolCheck` (primitive 9) generalises this pattern; ad-hoc project-scoped lints still implement `Lint` directly with their own logic if their analysis does not fit primitive 9.

## Shared engine utilities

Several v1 pain points dissolve into shared utilities that primitives use uniformly.

**Boundary matching.** One helper `boundary_match(haystack: &str, needle: &str) -> impl Iterator<Item = Match>` that yields positions where `needle` appears with word boundaries on both sides. Every TokenScan instance uses it. Fix once.

**Source stripping.** `MockspaceDocument::source_stripped(opts: StripOpts) -> &str` with cached lazy computation per `(document, opts)` pair. Each lint declares what it needs via scope config (`scope.strip = { strings = true, comments = true, doc_comments = true }`) and the engine produces the relevant view from the cache. Lints never strip manually.

**AST cache.** Engine parses each Rust document once with `syn::parse_file` on first call to `doc.ast()`. Result is cached on the Document. Primitives 2, 3, 4 read `doc.ast()` (for syn-tree work) or `doc.tree_sitter()` (for tree-sitter work; some primitives need both views). Failed parses cache `None` and the lint either skips or emits a parse-failure finding depending on its declared behaviour.

**Suppression filtering.** Engine applies SuppressionMap to the returned `Vec<Finding>` automatically. Lints never read suppression themselves. Only primitive 11 (SuppressionMeta) reads the map, and it does so via an engine accessor, not by parsing source.

## TOML schema sketch

Consumer `lints.toml` at the project root. Schema matches viola TOML v2; renames to `viola.toml` post-integration without schema change.

Top-level:

```toml
[lints]
default_severity = "warn"

[lints.scope]
exempt_paths = ["target/**", "**/.git/**"]
proc_macro_exempt_by_default = true
```

Per-lint blocks:

```toml
[lints.no-bare-vec]
kind = "ast-type-position"
description = "Forbid bare std collection types in public APIs."
impact = "major"
category = "correctness"

[lints.no-bare-vec.config]
forbidden_types = ["Vec", "HashMap", "HashSet", "VecDeque", "BTreeMap", "BTreeSet", "LinkedList"]
positions = ["struct-field", "fn-param", "fn-return", "type-alias-body"]
visibility = "public"
strip_strings = true
strip_comments = true

[lints.no-bare-vec.scope]
paths = ["**/*.rs"]
exempt_paths = ["**/ffi/**"]
exempt_categories = ["bare-collection"]
proc_macro_exempt = true

[lints.no-bare-vec.gate.commit]
severity = "warn"
only_staged = true
[lints.no-bare-vec.gate.build]
severity = "error"
[lints.no-bare-vec.gate.push]
severity = "error"
```

Worked example: a content lint with dead-term-to-replacement table.

```toml
[lints.vocabulary-discipline]
kind = "term-replacement-table"
description = "Replace dead workspace terms with the canonical replacements."
impact = "minor"
category = "consistency"

[lints.vocabulary-discipline.config]
word_boundary = true

[lints.vocabulary-discipline.config.replacements]
"substrate" = "foundations"
"HList" = "cons-list"
"chain" = "fiber"
"partition" = "phase"
"entity" = "record"

[lints.vocabulary-discipline.scope]
paths = ["**/*.md.tmpl", "**/*.md", "**/*.rs"]
crates = ["hilavitkutin*"]

[lints.vocabulary-discipline.gate.commit]
severity = "warn"
[lints.vocabulary-discipline.gate.push]
severity = "error"
```

Worked example: a project-scoped lint.

```toml
[lints.no-duplicate-fn]
kind = "cross-doc-symbol"
description = "Forbid duplicate public fn names across crates."
impact = "major"
category = "consistency"

[lints.no-duplicate-fn.config]
symbol_kind = "fn"
visibility = "public"
predicate = "no-duplicates-across-crates"

[lints.no-duplicate-fn.scope]
paths = ["**/*.rs"]
exempt_paths = ["**/tests/**", "**/benches/**"]

[lints.no-duplicate-fn.gate.build]
severity = "error"
[lints.no-duplicate-fn.gate.push]
severity = "error"
```

Adding a new "lint" is a TOML edit. No Rust write, no proc-macro registration, no compile cycle.

### Per-finding-kind severity overrides

Some v1 lints emit several finding kinds (e.g. `WritingStyle` emits `em-dash`, `marketing-word`, `filler`, `leading-list`). Per-finding-kind overrides ship as optional sub-blocks:

```toml
[lints.writing-style.gate.commit]
severity = "warn"

[lints.writing-style.gate.commit.finding_kinds]
"em-dash" = "error"
"marketing-word" = "warn"
"filler" = "hint"
```

Default falls back to the gate severity. Catalog declares the finding-kind set per lint so config validation can flag unknown kinds.

## Catalog mechanism

Mockspace-rs ships a default catalog: the set of `(name, kind, default_config, default_scope, default_severity)` tuples that `MockspaceEngine::new()` instantiates. Consumer `lints.toml` overrides any field per-lint, disables lints by setting all gate severities to `off` or `skip`, or adds new instances under any `kind = "..."` with custom config.

Catalog entry shape:

```rust
pub struct CatalogEntry {
    pub name: &'static str,
    pub description: &'static str,
    /// Open string discriminator. Stack-lints contributes additional kinds
    /// via static registration; mockspace-rs is not the only producer.
    pub kind: &'static str,
    pub default_config: toml::Table,
    pub default_scope: toml::Table,
    pub default_severity: GateSeverity,
    pub default_impact: Option<Impact>,
    pub default_category: Option<Category>,
    pub doc_url: Option<&'static str>,
    pub mode: LintMode,
    pub staging_aware: bool,
    /// Constructor function: produces a Box<dyn Lint> from validated config.
    pub instantiate: fn(&toml::Table, &toml::Table) -> Result<Box<dyn Lint>, ConfigError>,
    /// Optional finding-kind set this lint can emit (for per-kind severity validation).
    pub finding_kinds: &'static [&'static str],
}
```

The `kind` field is `&'static str` rather than a closed enum, so stack-lints (and future external lint packs) can contribute catalog entries whose `kind` strings live in their own crate. Mockspace-rs ships the built-in kinds (`token-scan`, `ast-node-position-match`, `ast-type-position`, `identifier-pattern`, `content-regex`, `term-replacement-table`, `file-metric`, `undocumented-item`, `cross-doc-symbol`, `workflow-state`, `suppression-meta`). The catalog merges entries from all registered sources at engine construction.

The instantiation flow:

1. Engine loads `lints.toml`.
2. For each `[lints.<name>]` block: resolve catalog entry by `kind`. Merge defaults with consumer overrides. Pass the resulting tables to `entry.instantiate`.
3. Validate. Bad config produces a `ConfigError`, accumulated into a `Vec<ConfigError>` separate from `Vec<Finding>`. Engine refuses to dispatch if any errors accumulated.
4. Construct the primitive instance via `entry.instantiate`.
5. Push onto the active lint list.
6. Lints with `gate.<active>.skip = true` are dropped from the active list. Lints with all gates silent (`off` or `skip`) at the active gate are dropped.

Override cascade: workspace-level `[lints]` defaults, then per-lint `[lints.<name>]`, then CLI flags. **CLI `--scope <crate>` intersects with existing scope, not overrides.** If a lint declares `scope.crates = ["arvo"]` and CLI passes `--scope hilavitkutin`, the lint runs on `intersect(["arvo"], ["hilavitkutin"]) = []` and is dropped silently with a verbose-log explanation. This preserves per-lint scope intent while letting CLI narrow the run.

### External lint-pack loading

Build-time only. Stack-lints contributes via Cargo dep plus static registration (using `inventory` or `linkme` distributed slices). Consumer Cargo.toml lists `mockspace-hilavitkutin-stack-lints` as a dep; the engine collects catalog entries from the distributed slice at startup. No dynamic plugin loading at runtime. The `LINT_CONTRACT_VERSION` constant exists for future-proofing but is not consumed in v2.

This decision retires the v1 `[lint-crates]` Git-dep loading mechanism in favour of the cleaner cargo-dep + static-registration path. The viola era will introduce a proper runtime plugin SDK; until then, all lint packs ship as cargo deps.

## Config errors (separate channel from Findings)

Bad config produces `ConfigError`, not `Finding`:

```rust
pub struct ConfigError {
    pub lint_name: String,
    pub field_path: String,
    pub kind: ConfigErrorKind,
    pub message: String,
    pub source_location: Option<Span>,  // pointing into lints.toml
}

pub enum ConfigErrorKind {
    UnknownField,
    TypeMismatch,
    InvalidValue,
    ContradictsCatalog,  // e.g. only_staged = true on ProjectScoped lint
    UnknownKind,
}
```

The engine returns `Outcome<Vec<Box<dyn Lint>>, Vec<ConfigError>>` from instantiation. CI fails on the error vec. Formatter renders config errors with their lints.toml span when available. Keeping `Finding` clean of synthetic kinds preserves the rule-id-links-to-doc invariant.

## Engine contracts mockspace-rs ships

The surface mockspace-rs adds in Phase 2D, on top of the scaffold already shipped:

1. **CatalogEntry registry.** A `catalog::default_entries() -> Vec<CatalogEntry>` function that collects mockspace built-ins plus statically-registered external entries (stack-lints).
2. **TOML loader.** `LintsConfig::load(path: &Path) -> Outcome<LintsConfig, ConfigError>` and `LintsConfig::instantiate(&self, catalog: &[CatalogEntry]) -> Outcome<Vec<Box<dyn Lint>>, Vec<ConfigError>>`. Two channels.
3. **StagingFilter.** Per-run set of staged paths plus the per-gate git query implementation. Constructed once at engine entry. Carries the env-var override for push-gate.
4. **MockspaceDocument AST caches.** `doc.ast()` returning `Option<&syn::File>`, `doc.tree_sitter()` returning `Option<&tree_sitter::Tree>`. Computed on first call per document. Engine pre-warms the cache before lint dispatch if any active lint declares `needs_ast = true`.
5. **MockspaceDocument source views.** `doc.source_stripped(opts: StripOpts) -> &str` cached per (Document, opts) pair.
6. **Catalog validation.** A `validate_config(entry, user_overrides) -> Result<toml::Table, Vec<ConfigError>>` step run at instantiation. Refuses contradictions (e.g. `only_staged = true` on `staging_aware = false` primitives), unknown keys, type mismatches, unknown finding-kind names in per-kind severity blocks.
7. **CLI override injection.** A `LintsConfig::apply_cli_overrides(&mut self, overrides: CliOverrides)` step run after load, before instantiate. `--scope` intersects; `--lint <name>` filters the active list; `--fix` is a flag honoured by the runner not the catalog.

The foundation crate (mockspace-core) does not change. All seven new surfaces are mockspace-rs-internal.

## Pain-point resolution table

| v1 pain | v2 resolution | Where it lives |
|---|---|---|
| Boundary check reinvented per lint | `boundary_match` shared utility | mockspace-rs utilities |
| String / comment stripping inconsistent | `MockspaceDocument::source_stripped(opts)` cached | mockspace-rs cache |
| AST reparsed per lint per file | `MockspaceDocument::ast()` / `.tree_sitter()` cached | mockspace-rs cache |
| Per-lint hardcoded crate scoping | `scope.crates` TOML field | engine pre-filter |
| Hardcoded `primitive-introductions` exemption | `scope.exempt_categories` TOML field | engine pre-filter |
| `lint:allow(...)` parsed per lint | Engine applies SuppressionMap automatically | mockspace-core (already shipped) |
| Two parallel trait hierarchies | One `Lint` trait, three `LintMode` values, two methods | mockspace-rs unified |
| No file-glob exemption | `scope.exempt_paths` globs via `globset` | engine pre-filter |
| No staging awareness | `gate.<g>.only_staged` plus `StagingFilter` | engine per-run |
| `--scope <crate>` bolted on | CLI override intersects existing scope | CLI layer |
| Config errors panic mid-run | Validate-then-dispatch at load; `Vec<ConfigError>` channel | catalog validation |
| No `rule_id` linking to docs | `CatalogEntry::doc_url` populates `Finding::rule_id` | catalog + CLI |
| `--fix` machinery absent | Contract ready (`FixSuggestion`), command deferred | Phase 5 |
| Severity downgrade requires Rust edit | All severities in TOML; per-finding-kind overrides | catalog + lints.toml |
| Per-repo / stack-lints overlap | 5 safe drops + 3 careful merges + 4 genuine keepers | consumer PR |

## Migration order

The order of operations from the existing migration plan applies, with primitive-count and bespoke-count revisions:

1. **Lock the schema design memo.** Per-primitive config grammars, scope schema, gate schema, mode declarations, catalog entry shape, override cascade rules, staging filter API (incl. detached-HEAD fallback), AST cache and source view contracts, suppression handoff, config validation flow, per-finding-kind severity grammar, external-lint-pack registration mechanism. Land as `mock/research/<timestamp>_lint-schema-design.md`.

2. **Implement the eleven reusable primitives.** One file each in `mockspace-rs/src/builtins/`. Per-primitive: `Lint` impl, `Config` deserialiser, `instantiate` constructor function, unit tests covering positive case, negative case, exemption case, scope variation case. Catalog entries land alongside.

3. **Implement the six bespoke primitives.** `no_bare_vec`, `no_manual_id`, `no_manual_impl`, `no_adhoc_framework`, `registrable_completeness`, `deprecation_comparison`. Same structure, less generic config. (`forbidden_imports` decision deferred to schema memo: ships as multi-`TokenScan` config or as bespoke seventh.)

4. **Author the default catalog.** `mockspace-rs/src/catalog.rs` ships the default `CatalogEntry` list plus the static-registration hook so stack-lints can contribute.

5. **Port mockspace built-ins.** The 16 lints staying built-in become catalog entries with appropriate config. Their old per-file implementations delete. Stack-lints migration runs in parallel: 17 lints become catalog entries in the stack-lints contribution.

6. **Per-repo `mockspace.toml` to `lints.toml` extraction.** Each consumer repo's `[lints.*]` blocks move to a new `lints.toml`. Field shape changes to add `kind = "..."` and the primitive-specific config sub-fields. Five safe duplicates delete; three merges delete after the migration PR verifies broader-scope behaviour ships; four genuine keepers stay as repo-local catalog entries.

7. **Verification.** `cargo mock check` in each consumer repo. Catch drift early.

Steps 2 and 3 are parallelizable across primitives. Steps 5 and 6 fan out per repo.

## Resolved-from-review items

Marking the senior-review findings inline so the schema memo does not relitigate them:

| # | Finding | Resolution |
|---|---|---|
| 1 | AST cache trait location | Concrete `MockspaceDocument`; foundation trait untouched |
| 2 | PerDocument wrapper cost | Two methods on trait (`check_document` + `check_project`) |
| 3 | `ForbiddenTokens` covers ~19 lints | Split into `TokenScan` + `AstNodePositionMatch` + `AstTypePosition` |
| 4 | Push-gate detached HEAD | Env override + `@{upstream}` + full-project fallback |
| 5 | "8 duplicates" claim | Corrected: 5 safe + 3 merge + 4 keepers |
| 6 | `StructFieldShape` ⊂ `FnSignatureShape` | Both collapse into `AstTypePosition` |
| 7 | `ContentRegex` for `vocabulary-discipline` | New primitive `TermReplacementTable` |
| 8 | `visibility` hidden coupling | Per-primitive config field, rejected in scope block |
| 9 | `exempt_categories` granularity | Schema memo names per-token vs per-document semantics |
| 10 | CLI `--scope` semantics | Intersect, not override |
| 11 | Pass-1-all + Pass-2-staged | Supported; only_staged applies only to Pass 2 in TwoPhase |
| 12 | LSP/editor gate mapping | Editor bypasses staging; runs on edited buffer with commit-gate severities |
| 13 | Per-finding-kind severity | `[lints.<name>.gate.<g>.finding_kinds.<kind>]` sub-block |
| 14 | External lint-pack loading | Cargo dep + static registration (`inventory`/`linkme`) |
| 15 | "substrate" usage | Replaced with "foundations" / "engine internals" per vocabulary rule |
| 16 | Em-dashes outside code fences | Verified clean; literal regex `—` inside code fence demonstration is correct |
| 17 | Config-validation Finding shape | Separate `Vec<ConfigError>` channel |
| 18 | Catalog `BuiltinKind` enum closed | `kind: &'static str` open; static registration adds entries |

## Risks and remaining open questions

The schema memo locks the items still open:

1. **Glob library syntax**. `globset` chosen; lock the documented syntax for `**`, `*`, brace expansion, and `!` negation.

2. **`SemanticAliasNudge`** fits `AstTypePosition` with a hint-severity plus per-type suggestion text. The "suggest replacement" extension on `AstTypePosition` versus a separate primitive is the same shape question `TermReplacementTable` resolved for content lints. Schema memo decides whether `AstTypePosition` grows a replacement table or a separate `TypePositionReplacement` primitive ships.

3. **`WorkflowState` coupling.** Reads the typestate layer in mockspace-core. Mockspace-rs depends on mockspace-core for the typestate; expand the dep. Alternative (move to thin `mockspace-workflow-lints` crate) is less attractive: typestate is workflow-foundational and the lint is its companion.

4. **`forbidden_imports` shape.** Multi-rule data engine: ship as multiple `TokenScan` instances with a config-load helper that expands the namespaced TOML, or as bespoke seventh. Schema memo decides.

5. **`deprecation_comparison` vs `WorkflowState`.** Both touch cross-CL state; consolidation or separation is a primitive-set design question. Schema memo decides.

6. **`design_doc_source_mismatch` vs `CrossDocSymbolCheck`.** Borderline; could absorb into `CrossDocSymbolCheck` with a `must-appear-in-design-doc` predicate plus a side hook for SHAME-entry escape. Schema memo decides.

7. **Performance of staging at large repos.** `git diff --name-only --cached` is fast (sub-second) on most repos; on very large repos can be hundreds of ms. Acceptable for pre-commit; not for editor LSP loop. The editor surface (resolved above) skips staging entirely.

8. **Test fixture format.** Each primitive needs a fixture-based test runner: input source plus config plus expected `Vec<Finding>`. Probably TOML. Schema memo locks the format.

9. **`Lint::default_severity()` vs `[lints.<name>.gate.<g>].severity`.** Two sources of truth in the trait surface plus catalog. Resolution: the trait method is informational (callers use it for diagnostic display); the catalog `default_severity` is the source of truth at instantiation. Schema memo documents.

10. **`LintError` type.** Returned from `check_document` / `check_project`. Concrete shape:

    ```rust
    pub enum LintError {
        /// Parse failure on a document the lint required (e.g. AST primitive on un-parseable Rust).
        ParseFailure { path: PathBuf, source: String },
        /// Internal invariant violation in the lint impl.
        Internal(String),
        /// I/O failure reaching workflow state (WorkflowState primitive).
        WorkflowIo(io::Error),
    }
    ```

    Engine catches `LintError` per dispatch and converts to a diagnostic Finding tagged with the lint name. The run continues with the remaining lints; one lint's failure does not block the rest. Schema memo finalises any additional variants.

11. **Parallelism model.** PerDocument lints are trivially parallel; ProjectScoped are not. Default: per-lint sequential within a document, per-document parallel across lints (rayon-driven). Schema memo locks the threading shape.

12. **Findings carry `Span`s but v1 lints emit line numbers.** Migration mapping: catalog instantiate functions translate line numbers into spans against the document source. Schema memo locks the conversion shape.

13. **CLI `--lint <name>` flag.** Engine reads the flag, drops all other lints from the active set before dispatch. Trivial but worth naming in the memo for completeness.

14. **Consumer-repo TOML format change migration window.** Adding `kind = "..."` to every `[lints.<name>]` block is mechanical but per-repo. If mockspace ships v2 before stack-lints catches up, consumer repos have a transient window where their stack-lints pack is incompatible with the new catalog format. Mitigation: tag mockspace and stack-lints releases close together and update the workspace memo so consumer-repo updates land bundled.

## What the schema design memo locks

The follow-up memo (the immediate next artifact after this one re-reviews) settles:

1. Per-primitive `Config` schemas (the `[lints.<name>.config]` shape for all 11 reusable primitives + 6 bespoke).
2. Scope schema (the unified `[lints.<name>.scope]` shape; visibility-not-in-scope rule).
3. Gate schema (`severity`, `only_staged`, `skip` per gate; per-finding-kind sub-blocks).
4. Mode declarations (which primitive uses which `LintMode`; per-phase staging for TwoPhase).
5. `CatalogEntry` shape, including `staging_aware`, `mode`, `doc_url`, `kind: &'static str`, `instantiate`, `finding_kinds`.
6. Override cascade rules (workspace defaults to per-lint to CLI; intersect semantics).
7. Staging filter API (per-gate git query, env-var fallback, integration with `LintContext`).
8. AST cache and source view contracts on `MockspaceDocument`.
9. Suppression handoff (engine filters automatically; primitive 11 reads the map via engine accessor).
10. Config validation flow and `ConfigError` shape (separate channel).
11. Glob library and syntax.
12. Test fixture format.
13. CLI override semantics (`--scope` intersect, `--lint` filter, future `--fix`).
14. External lint-pack registration mechanism (static-registration choice).
15. Parallelism model (per-lint sequential within document; per-document parallel across lints).
16. Editor surface behaviour (bypass staging; commit-gate severities).
17. `MockspaceProject` and `MockspaceDocument` concrete-type contracts.

A few of these (5, 7, 8, 17) inform mockspace-rs's surface; the rest are TOML plus engine-internal. None touch the foundation crate.

## References

- Reviewer findings: `mock/research/202605202300_lint-primitive-proposal-review.md` (18 findings; 17 resolved inline above, 1 verified clean).
- Verified per-lint catalog: `mock/research/202605210000_lint-corpus-mechanism-audit.md` (37 Pool A + 18 Pool B + 12 Pool C = 67 entries with file:line citations).
- Earlier audit: `mock/research/202605201500_lint-catalog-migration-plan.md` for the 63-lint pool categorisation. Deletion-count revision lands during the consumer-repo migration PR.
- Foundation contract: `mock/research/202605201700_engine-preprocessor-architecture.md`. The foundation crate stays untouched.
- Viola engine integration: `mock/research/202605201400_viola-engine-integration-shape.md`. The primitive set becomes the viola-plugin set when viola lands; ratio is 17 (11 reusable + 6 bespoke), not 63.
- Workspace rule on policing vs tools: `.claude/rules/arvo-toolbox-not-policer.md`. The primitive set ships sharp tools; consumers compose via TOML. Lint authoring is a tool choice, not a policy decision.

## Recorded

- **2026-05-20.** Authored after a 63-lint shallow audit revealed that most of the corpus collapses into a small primitive set and that v1's operational pain points have shared root causes the consolidation can resolve. Successor to (not replacement for) the catalog migration plan; the destination categorisation in that plan still holds.
- **2026-05-21 (Revision 2).** Senior review at `202605202300_lint-primitive-proposal-review.md` falsified the 7+3 primitive count and the 8-duplicate claim. Per-lint mechanism audit at `202605210000_lint-corpus-mechanism-audit.md` walked all three pools line-by-line and produced the verified 11+6+1 count. This revision threads the audit's verified counts and the reviewer's structural decisions inline. The 18 review findings are resolved or surfaced to the schema memo per the table above.
- **2026-05-21 (Revision 2 follow-up).** Second-pass senior review caught two load-bearing defects the first revision asserted without designing: (D1) `staging_aware` was a binary flag but Pass-1-all/Pass-2-staged needed mode-dependent meaning; (D3) `MockspaceProject` was referenced everywhere but never enumerated. Both resolved inline. `staging_aware` now has documented mode-dependent semantics with a concrete catalog values table. `MockspaceProject` surface members (`documents`, `staged_documents`, `crate_graph`, `workspace`, `design_rounds`, `suppressions`, `surface`, `gate`, `introduced_categories`) are enumerated as the surface lints read through. `LintError` shape (D4) committed inline. Schema-memo readiness: ready.
