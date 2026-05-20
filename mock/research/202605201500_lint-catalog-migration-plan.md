# Lint catalog migration plan

**Date:** 2026-05-20
**Status:** Proposal, pre-implementation
**Scope:** Mockspace v2 Phase 2D, lint catalog migration to the viola-native substrate
**Sibling notes:**
- `mock/research/202605201400_viola-engine-integration-shape.md`
- `mock/research/202605201700_engine-preprocessor-architecture.md` (post-revision; supersedes the substrate trait surface this note's cookbook references; see addendum below)

> **Addendum (2026-05-20):** the revised architecture note removes `Lint` / `PerDocumentLint` from the substrate and moves them to mockspace-rs as engine-internal authoring traits. The Pattern A/B/C/D cookbook below still applies mechanically; the trait names target mockspace-rs's authoring surface instead of mockspace-core's substrate. The destination categorisation (16 stay built-in to mockspace-rs's catalog, 17 move to stack-lints, 8 drop as duplicates, 3 stay repo-local) is unchanged. Consumer-side migration also extracts lint config from `mockspace.toml` into a new `lints.toml` file at the project root (schema matches viola TOML v2; renames to `viola.toml` post-viola-integration). See the architecture note §6.

## Why this note exists

The v2 substrate reshape landed `LINT_CONTRACT_VERSION = 2` with new traits: project-scoped `Lint`, the `PerDocumentLint + PerDocumentLintBlanket` convenience, the `Document` / `Project` / `Language` / `ContentHash` / `RunSurface` vocabulary, and a single `LintEngine` trait with `MockspaceEngine` as placeholder.

The existing catalog (built before this reshape) targets v1. 63 lints sit across three pools. They need migration. The substrate's "minimal downstream churn" promise needs a concrete plan, not just a stable trait.

This note categorises every existing lint into a destination (stays built-in / moves to the shared stack-lints pack / drops / merges with a duplicate), names the trait migration shape per category, and orders the work so consumer repos see one migration cycle, not three.

## The two questions

1. **Where should each lint live?** Mockspace built-in (because it understands workflow artefacts), shared stack-lints pack (because it enforces workspace-wide discipline), or repo-local (because the rule applies only to one consumer).
2. **What does the v1 → v2 migration look like per lint?** A mechanical mapping that consumer crates and the lint-pack maintainer execute once.

The first question is editorial; the second is mechanical. They are independent. Answering the first decides what each lint's `Cargo.toml` looks like and which repo holds the source; answering the second is the same migration recipe regardless of which crate the lint ends up in.

## Inventory

### Pool 1: mockspace built-ins

Source: `mockspace/lint-rules/src/`. Trait surface: v1 `Lint` + v1 `CrossCrateLint`. 33 lints total (25 `Lint` + 8 `CrossCrateLint`).

**Mockspace-workflow-specific** (understand CLs, design rounds, manifest grammar):
- `changelist_doc_gate` (CrossCrateLint)
- `changelist_lock` (CrossCrateLint)
- `changelist_required` (CrossCrateLint)
- `changelist_immutability` (CrossCrateLint)
- `design_doc_source_mismatch`
- `deprecation_comparison` (CrossCrateLint)
- `single_source` (CrossCrateLint)
- `registrable_completeness`

**Workspace-discipline** (enforce stack vocabulary, would apply to any stack consumer):
- `no_bare_string`
- `no_bare_vec`
- `no_bare_result`
- `no_bare_macro_types`
- `no_float`
- `no_self_define`
- `no_adhoc_framework`
- `no_pool_access`
- `no_box`
- `no_raw_error_outside_primitives`
- `repr_c_abi_safety`
- `no_manual_impl`
- `no_entry_suffix`
- `no_manual_id`
- `no_primitive_key`
- `no_adhoc_error_enum`
- `forbidden_imports`

**Generic content lints** (language-agnostic hygiene):
- `no_todo`
- `file_size`
- `actionable_errors`
- `export_count`
- `undocumented_type` (CrossCrateLint)
- `no_bare_pub`
- `no_duplicate_fn` (CrossCrateLint)
- `no_empty_crate`

### Pool 2: shared stack-lints pack

Source: `mockspace-hilavitkutin-stack-lints/src/lints/`. Trait surface: v1, pinned to mockspace `main`. 18 lints.

- `NoAlloc`
- `NoStd`
- `NoBareOption`
- `NoBareResult`
- `NoBareNumeric`
- `NoBareString`
- `NoBareStaticStr`
- `NoDynDispatch`
- `NoRuntimeSpawn`
- `NoRuntimeRegistration`
- `NoPublicRawField`
- `NoVecInTraitSig`
- `StrategyMarkerRequired`
- `SemanticAliasNudge`
- `TraitFirstSignatures`
- `ArvoTypesOnly` (explicitly retained for backward config compatibility, redundant with `NoBareNumeric`)
- `LintAllowRequiresTaskId`
- `WritingStyle` (CrossCrateLint, scans `*.md.tmpl` + rustdoc comments)

### Pool 3: per-repo custom lints

Source: `<repo>/mock/lints/`. Trait surface: v1.

**arvo** (6 lints):
- `no_std_enforcer` (duplicate of stack-lints `NoStd`)
- `no_alloc_enforcer` (duplicate of stack-lints `NoAlloc`)
- `no_dynamic_dispatch` (duplicate of stack-lints `NoDynDispatch`)
- `strategy_marker_required` (duplicate of stack-lints `StrategyMarkerRequired`)
- `arvo_bits_traits_only` (arvo-specific facade discipline)
- `no_runtime_grow` (`.push` / `.resize` / `.extend` ban; arvo-shaped no-heap discipline)

**hilavitkutin** (6 lints):
- `no_std_enforcer` (duplicate)
- `no_alloc_enforcer` (duplicate)
- `no_dynamic_dispatch` (duplicate)
- `no_runtime_spawn` (duplicate of stack-lints `NoRuntimeSpawn`)
- `no_runtime_registration` (duplicate of stack-lints `NoRuntimeRegistration`)
- `vocabulary_discipline` (hilavitkutin-specific dead-term tracking)

**notko / viola / viola-grammar-ts / vehje**: no per-repo lints. Consume the shared stack-lints pack via `[lint-crates]`.

## Categorisation: destination per lint

The principle: **a lint goes where its target audience lives**. Mockspace's workflow lints understand mockspace artefacts; they ship with the binary. Workspace-discipline lints enforce rules that apply to every consumer crate; they live in the shared pack so consumers get them by adding one `[lint-crates]` entry. Repo-specific lints stay in the repo when their rule genuinely does not generalise.

### Destination: stays mockspace built-in

| Lint | Why |
|---|---|
| `changelist_doc_gate` | Parses doc CL syntax; mockspace workflow only. |
| `changelist_lock` | Validates locked-CL semantics; mockspace workflow only. |
| `changelist_required` | Pre-merge ceremony; mockspace workflow only. |
| `changelist_immutability` | Locked-CL immutability; mockspace workflow only. |
| `design_doc_source_mismatch` | Cross-checks design doc claims against source; mockspace workflow. |
| `deprecation_comparison` | Tracks deprecation chains in `mock/design_rounds/`; mockspace workflow. |
| `single_source` | Single-definition rule across crates; mockspace coherence. |
| `registrable_completeness` | Extension-point registration coverage; mockspace contract. |
| `no_todo` | Generic content; no Rust-substrate dependency. |
| `file_size` | Generic content; no Rust-substrate dependency. |
| `actionable_errors` | Generic content; applies to any language. |
| `export_count` | Generic API-surface hygiene. |
| `undocumented_type` | Generic docs discipline. |
| `no_bare_pub` | Encapsulation; language-agnostic by intent. |
| `no_duplicate_fn` | Cross-crate name collision; generic. |
| `no_empty_crate` | Workspace ergonomics; generic. |

These 16 lints share one property: they apply to consumers regardless of which stack the consumer is built on. A future non-substrate consumer of mockspace (say, a documentation-only repo) still wants `no_todo` and the changelist family. Built-in is the right home.

### Destination: moves to mockspace-hilavitkutin-stack-lints

| Lint | New name (suggested) | Why |
|---|---|---|
| `no_bare_string` | `NoBareString` (already exists; consolidate) | Stack vocabulary. |
| `no_bare_vec` | `NoBareVec` (new in stack-lints) | Stack vocabulary. |
| `no_bare_result` | `NoBareResult` (already exists; consolidate) | Stack vocabulary. |
| `no_bare_macro_types` | `NoBareMacroTypes` (new) | Stack vocabulary, proc-macro context. |
| `no_float` | rolled into `NoBareNumeric` | Stack vocabulary, primitive scope. |
| `no_self_define` | `NoSelfDefine` (new) | Use-the-stack discipline. |
| `no_adhoc_framework` | `NoAdhocFramework` (new) | Use-the-stack discipline. |
| `no_pool_access` | `NoPoolAccess` (new) | Substrate discipline. |
| `no_box` | rolled into `NoAlloc` | Substrate discipline, alloc-adjacent. |
| `no_raw_error_outside_primitives` | `NoRawErrorOutsidePrimitives` (new) | Substrate discipline. |
| `repr_c_abi_safety` | `ReprCAbiSafety` (new) | FFI discipline; applies to any FFI-shipping stack consumer. |
| `no_manual_impl` | `NoManualImpl` (new) | Substrate trait discipline. |
| `no_manual_id` | `NoManualId` (new) | Semantic-alias discipline. |
| `no_primitive_key` | `NoPrimitiveKey` (new) | Semantic-alias discipline. |
| `no_adhoc_error_enum` | `NoAdhocErrorEnum` (new) | Substrate error-type discipline. |
| `forbidden_imports` | `ForbiddenImports` (new) | Substrate; forbids `std::*` / `alloc::*` in consumer code. |
| `no_entry_suffix` | drop OR generalise (see below) | Originally clause-shaped; arguably workspace-naming-disciple. |

These 17 lints share one property: their target audience is "a stack consumer crate". Moving them to the shared pack lets consumer repos pick them up by adding one `[lint-crates]` entry instead of every repo re-declaring builtin enablement in its own `mockspace.toml`.

### Destination: per-repo deduplication

Drop these duplicates entirely once the stack-lints pack ships v2:

**arvo/mock/lints/**:
- `no_std_enforcer` → use stack-lints `NoStd`
- `no_alloc_enforcer` → use stack-lints `NoAlloc`
- `no_dynamic_dispatch` → use stack-lints `NoDynDispatch`
- `strategy_marker_required` → use stack-lints `StrategyMarkerRequired`

**hilavitkutin/mock/lints/**:
- `no_std_enforcer` → use stack-lints `NoStd`
- `no_alloc_enforcer` → use stack-lints `NoAlloc`
- `no_dynamic_dispatch` → use stack-lints `NoDynDispatch`
- `no_runtime_spawn` → use stack-lints `NoRuntimeSpawn`
- `no_runtime_registration` → use stack-lints `NoRuntimeRegistration`

These eight duplicate lints exist because the per-repo lints predate the shared pack (or were written in parallel without dedup). The migration is purely deletion plus `mockspace.toml` config delta to enable the stack-lints equivalents.

### Destination: stays per-repo

| Repo | Lint | Why |
|---|---|---|
| arvo | `arvo_bits_traits_only` | Targets one arvo crate (`arvo-bits` facade discipline); does not generalise. |
| arvo | `no_runtime_grow` | arvo's specific list of forbidden growth methods; the policy is arvo-shaped. Could move if hilavitkutin and vehje want the same rule; punt for now. |
| hilavitkutin | `vocabulary_discipline` | Hilavitkutin-specific dead-term list (`chain`, `partition`, `entity`, `row`, `order`); not workspace-wide. |

These three are genuinely repo-shaped. Their rules do not apply outside the repo that ships them; centralising them in stack-lints would force unrelated repos to opt out.

### Destination: drop or revisit

| Lint | Action | Reason |
|---|---|---|
| `arvo_types_only` (stack-lints) | drop after v2 ships | Explicitly redundant with `NoBareNumeric`; the lint pack already notes this. |
| `no_entry_suffix` (mockspace) | drop OR generalise | The "no `*Entry` suffix" rule was clause-specific naming policy. If still desired workspace-wide, rename and move to stack-lints; otherwise delete with the v2 migration. |
| `no_manual_id` (mockspace) | revisit during migration | Overlaps `no_primitive_key` and the semantic-alias discipline. Decide at migration time whether to keep both or merge. |

## Trait migration: v1 → v2 cookbook

Every existing lint, regardless of destination pool, maps to one of two shapes in v2.

### Pattern A: per-document lint (the majority)

A v1 lint that processes one source file at a time becomes a `PerDocumentLint`. The migration is mechanical:

**v1 shape** (illustrative):
```rust
impl Lint for NoTodo {
    fn name(&self) -> &'static str { "no-todo" }
    fn check(&self, source: &str, file: &Path, severity: Severity) -> Vec<Finding> {
        // scan source for `todo!()`; build findings
    }
}
```

**v2 shape**:
```rust
impl PerDocumentLint for NoTodo {
    fn name(&self) -> &'static str { "no-todo" }
    fn description(&self) -> &'static str { "Forbids `todo!()` in shipped source." }
    fn default_severity(&self) -> GateSeverity { GateSeverity::push_error() }
    fn check_document(
        &self,
        ctx: &LintContext<'_>,
        document: &dyn Document,
        sink: &mut dyn FindingSink,
    ) -> Result<(), LintError> {
        let source = document.source();
        let file = document.path();
        // scan source for `todo!()`; sink.emit(finding) per occurrence
        Ok(())
    }
}

impl PerDocumentLintBlanket for NoTodo {}
```

Three mechanical edits:
1. Trait name: `Lint` → `PerDocumentLint`. Add the marker `impl PerDocumentLintBlanket for X {}`.
2. Method signature: `check(source, file, severity)` → `check_document(ctx, document, sink)`. Read source from `document.source()`, path from `document.path()`, severity from `ctx.severities`.
3. Return shape: `Vec<Finding>` → emit findings via `sink.emit(...)` and return `Result<(), LintError>`.

The pattern-detection logic inside the function body does not change. The change is at the boundary.

### Pattern B: project-scoped lint (cross-document)

A v1 `CrossCrateLint` (and any new lint whose analysis spans documents) becomes a direct `Lint` impl. There is no separate `CrossCrateLint` trait in v2; one trait covers both shapes.

**v1 shape** (illustrative):
```rust
impl CrossCrateLint for NoDuplicateFn {
    fn name(&self) -> &'static str { "no-duplicate-fn" }
    fn check(&self, files: &[ParsedFile], severity: Severity) -> Vec<Finding> {
        // build map of fn name -> sites; emit findings on collisions
    }
}
```

**v2 shape**:
```rust
impl Lint for NoDuplicateFn {
    fn name(&self) -> &'static str { "no-duplicate-fn" }
    fn description(&self) -> &'static str { "Forbids duplicate function names across crates." }
    fn default_severity(&self) -> GateSeverity { GateSeverity::push_error() }
    fn check(
        &self,
        ctx: &LintContext<'_>,
        project: &dyn Project,
        sink: &mut dyn FindingSink,
    ) -> Result<(), LintError> {
        let mut seen: HashMap<&str, Vec<&Path>> = HashMap::new();
        for doc in project.documents() {
            for fn_name in extract_fn_names(doc.source()) {
                seen.entry(fn_name).or_default().push(doc.path());
            }
        }
        for (name, sites) in seen {
            if sites.len() > 1 {
                sink.emit(/* finding referencing all sites */);
            }
        }
        Ok(())
    }
}
```

The iteration moves into the lint body. `project.documents()` returns the slice; the lint chooses how to walk it. No blanket impl, no marker trait; direct `impl Lint for X`.

### Pattern C: language-conditional lints

Lints that only apply to certain source languages branch on `document.language()`:

```rust
fn check_document(&self, ctx: &LintContext<'_>, document: &dyn Document, sink: &mut dyn FindingSink) -> Result<(), LintError> {
    if document.language() != Language::Rust { return Ok(()); }
    // Rust-specific scan
    Ok(())
}
```

This replaces the v1 pattern of inspecting the file extension manually. The substrate carries the language tag; the lint reads it.

### Pattern D: surface-conditional lints

Lints that should fire only on CI (or only in editor mode) branch on `ctx.surface`:

```rust
fn check_document(&self, ctx: &LintContext<'_>, document: &dyn Document, sink: &mut dyn FindingSink) -> Result<(), LintError> {
    if !matches!(ctx.surface, RunSurface::Ci) { return Ok(()); }
    // CI-only scan
    Ok(())
}
```

This is new in v2. The v1 surface concept was implicit via gate severity; v2 exposes it as a first-class branch.

## What about `LintCfgStore` and severity resolution?

Each lint reads its TOML config from `ctx.config.get(lint_name)`. The default `resolve_severity` impl deserialises the lint's sub-table into `GateSeverity` if present; otherwise the engine falls back to `lint.default_severity()`. Lint authors do not call `resolve_severity` themselves; the engine wires this up before calling `check`. Lints read `ctx.severities` (the resolved per-gate triple) and `ctx.active_severity()` (the severity at the current gate) directly.

If a lint needs typed config beyond severity (e.g. a list of forbidden imports), it reads `ctx.config.get("forbidden-imports")` and deserialises the table into its own typed config struct via `toml::Table::try_into()`.

## Order of operations

The migration cycle from v1 to v2 spans multiple repos. Done correctly, each consumer repo touches one cycle. Done incorrectly, consumers touch three cycles (once when mockspace ships, once when stack-lints ships, once when their per-repo lints migrate).

The right order:

1. **Land Phase 2A + 2B inside mockspace** (this PR branch and the next). Substrate is locked at `LINT_CONTRACT_VERSION = 2`. Placeholder `MockspaceEngine` is sound and runs against a real disk-walked project. The substrate stops moving.

2. **Migrate mockspace built-ins to v2** (Phase 2D, this repo, no consumer impact). Port each lint that stays built-in (the 16 lints in the "stays mockspace built-in" table) to the v2 trait shape. Drop the v1 `Lint` + `CrossCrateLint` traits from `mockspace/lint-rules/` since nothing impls them anymore. Drop `no_entry_suffix` and the workspace-discipline lints from `mockspace/lint-rules/` since they're moving to stack-lints. Bump the mockspace version; tag release.

3. **Migrate stack-lints to v2 + absorb the workspace-discipline lints** (separate PR cycle on `mockspace-hilavitkutin-stack-lints`). Port the 18 existing stack-lints from v1 to v2 (same mechanical migration). Add the 17 lints moving from mockspace built-ins; consolidate duplicates (`NoBareString`, `NoBareResult` already exist). Drop `ArvoTypesOnly` (redundant with `NoBareNumeric`). Bump the stack-lints version; tag release. Update its `Cargo.toml` mockspace dependency to the v2 tag.

4. **Update per-repo `mockspace.toml` + `mock/lints/`** (one PR per repo: arvo, hilavitkutin; notko / viola / viola-grammar-ts / vehje get a smaller PR because they only update their `[lint-crates]` pin). Delete duplicate per-repo lints. Migrate the remaining repo-local lints (arvo's `arvo_bits_traits_only`, `no_runtime_grow`; hilavitkutin's `vocabulary_discipline`) to v2. Update `[lint-crates]` to pin the new stack-lints release. Update the per-repo `mockspace.toml` to enable the relocated stack-lints rules (the rules that were per-repo are now in the pack, but each repo opts in via mockspace.toml severities).

5. **Verify** by running `cargo mock` in each consumer repo against the new pack. Catch drift early.

Phases 2 and 3 can run in parallel after Phase 1 (mockspace v2 substrate) ships; per-repo updates in Phase 4 wait for both.

## What "minimal downstream churn" means here

Consumer repos see exactly one PR cycle that touches lints. They:

- Update one `[lint-crates]` pin in `mockspace.toml`.
- Delete duplicate `mock/lints/*.rs` files (arvo and hilavitkutin only; the rest are zero changes).
- Optionally migrate any genuinely repo-local lint to v2 (arvo: 2 lints; hilavitkutin: 1 lint; others: zero).
- Optionally enable the relocated stack-lints rules in `mockspace.toml` (one `[lints.no-self-define]` block, etc.).

No consumer crate Rust source changes. No re-shaping of any `[primitive-introductions]` blocks. No new fields added to `mockspace.toml` at the consumer level. The lint identities that consumer crates care about (per-lint names like `no-bare-result`, `no-alloc`, `strategy-marker-required`) stay stable across the migration; only the source location of the lint definitions moves.

The change pile is concentrated in two repos: `mockspace` itself (Phase 2D) and `mockspace-hilavitkutin-stack-lints` (the parallel migration). Consumer crates see the changes as bundled in a single dep-pin bump.

## What stays unchanged across the migration

- Lint names (`no-bare-result`, `no-alloc`, `no-todo`, `changelist-required`, etc.). The TOML config keys consumer repos use stay verbatim.
- Severity vocabulary (`Off`, `Info`, `Warn`, `Error`) and per-gate triple (`commit` / `build` / `push`).
- The `lint:allow(<name>): tracked: #N` inline comment form. The substrate honours it the same way; the comment-parsing logic moves with each lint (most use a shared helper which moves to mockspace-core).
- The `mockspace.toml` `[lints.<name>]` block shape.
- The mockspace bootstrap and hook generation flow.
- The agent-rules surface in `mock/agent/rules/` (the rules describing the lints).
- The consumer-facing CLI (`cargo mock check`, `cargo mock lock`, etc.).

## What changes across the migration

- `LINT_CONTRACT_VERSION` bumps from 1 to 2 (already done; pre-release, no consumers to break).
- The `Lint` trait shape (already shipped in v2 form on this PR branch).
- The `CrossCrateLint` trait disappears as a separate type (folded into `Lint` directly).
- The mockspace-core lint module now exposes the `Document` / `Project` / `Language` / `RunSurface` / `ContentHash` vocabulary; lint impls bind to these types instead of v1's `ParsedFile`-like type.
- `mock/lints/` per-repo files: most delete (duplicates), some migrate (genuinely repo-local rules).

## Cookbook entry: writing a new lint, post-migration

When a future agent or contributor writes a new lint:

1. **Decide scope.** Per-document or project-scoped? If per-document, use `PerDocumentLint + PerDocumentLintBlanket`. If project-scoped, `impl Lint` directly.
2. **Decide home.**
   - Understands mockspace artefacts (CLs, rounds, manifests, design-doc grammar)? → mockspace built-in (`mockspace/lint-rules/src/`).
   - Enforces stack vocabulary or substrate discipline (no-alloc, no-std, primitive vocabulary, FFI safety, etc.)? → shared stack-lints pack (`mockspace-hilavitkutin-stack-lints/src/lints/`).
   - Specific to one consumer repo (a naming rule that applies only to that repo's domain)? → that repo's `mock/lints/`.
   - Generic content / hygiene that applies broadly but is not mockspace-shaped? → mockspace built-in (generic content lint category).
3. **Implement.** Follow Pattern A or Pattern B above. Add `#[serde(rename_all = "kebab-case")]` to any typed config struct that deserialises from `ctx.config.get(name)`.
4. **Register.** Add the lint to its pack's registry function (`all_lints()` in mockspace, `lint_pack!` in stack-lints, the per-repo registration helper in repo-local packs).
5. **Default severity.** Match the rule's strictness. Substrate-correctness rules: `error` at all gates. Hygiene rules: `warn` at `push`, `info` at `commit`. Content suggestions: `info`-only. Document the choice in the lint's `description()`.
6. **Test.** Add a positive case (source the lint should flag), a negative case (source it should not), and a config-override case (severity tuned via mockspace.toml).

## Honest assessment

The migration is mechanical. Each lint is one file, one trait impl, one registration line. The work is shaped like 60-something small migrations, not three large ones, but it follows a single recipe.

The categorisation does real work: it consolidates eight duplicate lints into the shared pack, removes a clause-historical rule (`no_entry_suffix`) that no longer reflects active policy, and clarifies which rules belong to the stack-discipline pack versus the mockspace built-in pack. Consumer repos see a smaller `mock/lints/` (or none) and a clearer `mockspace.toml`.

The substrate is in the right shape for this work. The only risk is order-of-operations drift: if Phase 2D ships in mockspace before stack-lints catches up, consumers see a transient window where their stack-lints pack is incompatible with the new mockspace version. Mitigation: tag mockspace and stack-lints releases close together and update the workspace memo so consumer-repo updates land bundled.

## Recorded

2026-05-20 alongside the viola-engine-integration note refresh. Authored as the post-reshape design record for the lint catalog migration. The substrate trait surface this plan targets is documented in the sibling note; the placeholder engine landed at `6c21786`.

Future work: when viola ships its engine crate, this plan's "stays built-in to mockspace" set transparently runs through `ViolaEngine` via the same `dyn Lint` registration; the categorisation does not change. The migration is a one-time cost.
