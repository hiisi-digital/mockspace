# Lint catalog onto the WorkUnit-cdylib boundary

**Date:** 2026-05-25
**Scope:** mockspace task #610. Bridge between the 2026-05-20 lint catalog migration plan and the WorkUnit-cdylib boundary landed under task #609.
**Status:** design memo, first slice of #610. Subsequent slices write the topic + DOC CL once the path question below is settled.
**Source memos:** `mock/research/202605201500_lint-catalog-migration-plan.md` (the v1 to v2 migration plan, pre-#609), `hilavitkutin/mock/research/202605232100_workunit-cdylib-boundary.md` (the #609 boundary recommendation).

> Note on task IDs: `#NNN` references in this memo are workspace task IDs from the mockspace task tracker (under `mock/tasks/` and surfaced via `TaskList`), not GitHub issue numbers. The workspace task tracker is the authoritative planning surface for these IDs.

## Why this memo exists

The 2026-05-20 migration plan categorised every lint by destination (16 stay mockspace built-in, 17 move to stack-lints, 8 drop as duplicates, 3 stay repo-local) and named the v1 to v2 trait migration cookbook. It assumed lints run inside mockspace's process via `dyn Lint` / `PerDocumentLint`. That assumption was the pre-#609 state.

Post-#609, the design space includes a different placement: lints run as cdylibs loaded through `hilavitkutin-extensions`, dispatched as hilavitkutin WorkUnits, invoked across a `#[repr(C)]` vtable. Viola already does this for its grammar / runner / lint roles. The question #610 names is whether mockspace's lint catalog adopts the same boundary, and if so, what the mockspace-specific layer looks like.

The migration plan still applies for the v1 to v2 trait migration mechanics and the destination categorisation. This memo does not re-litigate those. It adds the cdylib-boundary axis the plan did not have.

## Two paths

**Path A: mockspace lints become viola plugins.** Mockspace's lint catalog ports to cdylibs that export `viola.lint.evaluate.v1` (`viola/mock/crates/viola-plugin-abi/src/vtable.rs:102`). Mockspace's `LintEngine` trait gets a `ViolaEngine` impl that drives the viola plugin host, dispatching lints through viola's existing infrastructure. The cdylib boundary is one vtable shape across mockspace and viola.

This aligns with task #198 (integrate viola as mockspace's primary lint runtime) and the locked sequence direction. The migration plan's "stays mockspace built-in" pool runs through `ViolaEngine` transparently; the destination categorisation does not change.

**Path B: mockspace ships its own vtable.** A separate `mockspace.lint.evaluate.v1` provider id with its own `#[repr(C)]` vtable matching mockspace's `Document` / `Project` / `Finding` vocabulary (`mockspace-core/src/lint.rs`). Mockspace plugin cdylibs export this provider. Viola lint plugins export viola's. The two boundaries are parallel and independent.

Path B preserves vocabulary fidelity: mockspace lints think in `Document` / `Finding`, viola lints think in `NamPayload` / `DiagnosticBatch`. Path A flattens both into the viola wire format and the mockspace-side `Document` / `Finding` types become host-side conversions before the call.

## Recommendation: Path A

Path A is the right shape for these reasons.

1. **Locked sequence pulls toward Path A.** Task #198 names viola as the eventual lint runtime for mockspace. Task #200 names migration of lint config from `mockspace.toml` to `viola.toml`. Path B builds a parallel cdylib protocol whose only consumer is mockspace's transitional engine, then deprecates it when #198 lands. Path A leans into the eventual end-state directly.

2. **Vocabulary translation is a host-side concern.** `mockspace-core::lint` already separates input vocabulary (`Document`, `Project`, `Language`, `ContentHash`) from output vocabulary (`Finding`, `Span`, `Severity`). The `LintEngine` trait is the swap point; engines own everything else. A `ViolaEngine` impl translates `Document` to `NamPayload` (or to the input the runner-extracted NAM payload provides) before the vtable call, and translates `DiagnosticBatch` back into `Finding` after. The translation is mechanical, lives in one place, and does not pollute the cdylib boundary.

3. **One cdylib protocol is simpler than two.** Lint authors target a single vtable shape regardless of whether their lint runs under mockspace or under viola directly. The `viola-plugin-abi` crate is the contract surface; both hosts consume it. Path B forces authors to pick which host they target or to export both vtables.

4. **The boundary memo already covers the work.** The #609 memo (sections "Cdylib lint contract", "Symbol export contract", "How AccessSet flows across the FFI boundary") names the symbol shape, the vtable layout, and the host-side dispatch pattern. None of that needs reauthoring for mockspace.

The recommendation is to merge mockspace's lint cdylib protocol with viola's, and to make `ViolaEngine` the production `LintEngine` impl. `MockspaceEngine` (the placeholder at `mockspace-core/src/engine.rs`) stays available as the in-process engine for tests and for the transitional window before viola integration lands.

## What this means concretely

### Mockspace-core stays as-is

`mockspace-core::lint` already separates substrate vocabulary from engine-internal authoring traits. No changes to `LintEngine` / `Finding` / `Document` / `Project` / `Severity` / etc. The substrate's job is to define the swap point and the wire vocabulary, both of which already exist at `LINT_CONTRACT_VERSION = 3`.

### Mockspace-rs ships the placeholder engine

The current `MockspaceEngine` placeholder (at `mockspace-rs/src/engine.rs:30`) stays. It runs the 16 stays-mockspace-built-in lints in-process via `dyn Lint`, returns `Vec<Finding>` with suppressions applied, and satisfies the `LintEngine` trait. Tests and the transitional CI window depend on this; deletion is post-viola-integration.

### Viola integration lands `ViolaEngine`

A new crate (working name `viola-mockspace-engine`, lives in viola's `mock/crates/`) implements `mockspace_core::lint::LintEngine` by routing every lint dispatch through viola's plugin host. The crate:

- Reads mockspace's `Project` and `Document` values from substrate.
- Translates them into viola's `NamPayload` (via a runner plugin, or by direct construction if the document is simple enough to bypass the runner step).
- Iterates configured lint plugins, invoking `viola.lint.evaluate.v1` on each.
- Collects `DiagnosticBatch` outputs and translates them back into `Vec<Finding>`.
- Applies the substrate's `SuppressionMap` to filter findings, identical to what `MockspaceEngine` does today.
- Returns the filtered `Vec<Finding>`.

This crate is the only point that touches the FFI boundary on mockspace's behalf. Mockspace-rs and mockspace-core stay vocabulary-pure.

### The 16 mockspace-built-in lints port to cdylibs

The migration plan's "stays mockspace built-in" pool (changelist family, design-doc-source-mismatch, deprecation-comparison, single-source, registrable-completeness, no-todo, file-size, actionable-errors, export-count, undocumented-type, no-bare-pub, no-duplicate-fn, no-empty-crate) port to viola plugin cdylibs. Each lint becomes one cdylib whose descriptor exports `viola.lint.evaluate.v1` with the lint's evaluator function captured in a `LintEvaluateVtable` static.

Plugin authors use `#[hilavitkutin_extensions_macros::export_extension]` to emit the descriptor. The evaluator function:

1. Reads `nam: *const NamPayload` for the file context.
2. Reads `lint_config_bytes: (*const u8, arvo::USize)` for the lint's TOML config.
3. Performs the lint's analysis (the v2 cookbook's `check_document` body, translated to operate on `NamPayload` instead of the substrate's `Document`).
4. Writes findings into `*mut DiagnosticBatch` using the host-provided write surface.
5. Returns `AbiStatus::Ok` (or an error code).

The mechanical translation from v1 `Lint::check(source, file, severity) -> Vec<Finding>` to v2-cdylib `unsafe extern "C" fn evaluate(...) -> AbiStatus` is the post-#609 evolution of the migration plan's Pattern A. The lint body's pattern-detection logic stays unchanged; the change is at the FFI boundary.

### The 17 workspace-discipline lints port to cdylibs in stack-lints

The migration plan's "moves to mockspace-hilavitkutin-stack-lints" pool ports the same way. Each lint becomes one cdylib in the shared pack repo. Consumer repos pick them up by listing the plugin paths in their per-repo viola config (post-`viola.toml` migration, task #200).

### Repo-local lints port the same way

`arvo_bits_traits_only`, `no_runtime_grow`, `vocabulary_discipline` follow the same pattern. They are repo-local cdylibs the consumer repo ships and configures.

## First slice scope

A first concrete PR slice translates one lint end-to-end as the proof of concept and the pattern reference for the rest. Recommended candidate: `no_todo`.

`no_todo` is the simplest lint in the "stays mockspace built-in / generic content" pool. Its pattern-detection logic is one regex-equivalent scan for `todo!()` occurrences. It has no cross-document state. It applies to any source language with minimal language-aware variation. Porting it to a cdylib exercises every piece of the boundary (descriptor export, vtable static, evaluator function, NamPayload reading, DiagnosticBatch writing) without dragging in lint-specific complexity.

The first-slice PR opens a topic + DOC CL in mockspace (or in viola, depending on where the per-lint cdylibs live) that:

- Names the cdylib crate (e.g. `mockspace-lint-no-todo` if it lives in mockspace, or `viola-lint-no-todo` if it lives in viola).
- Specifies the descriptor + vtable + evaluator shape.
- Identifies the wire-format reads the lint performs against `NamPayload` (file path, file content access, line numbers).
- Specifies the diagnostic emission shape (Severity mapping, span construction).
- Names the test-harness shape (does the cdylib run under viola's test fixtures, or does it have its own?).

Subsequent slices port the rest of the catalogue against the pattern the first-slice PR locks.

## Cdylib placement: one cdylib per pack

Three options for where per-lint cdylibs live: (1) each lint is its own crate under `mockspace/mock/crates/lints/<lint-name>/`; (2) all built-in lints live in one cdylib crate (`mockspace-builtin-lints`) that exports a `viola.lint.evaluate.v1` provider for each lint via the descriptor's `providers` table listing N entries; (3) lints live in `viola/mock/crates/lints/` because viola is the runtime they target. Each shape has tradeoffs. Option (2) minimises cdylib-load overhead at the cost of one per-author-choice crate boundary. Option (1) maximises author autonomy at the cost of N cdylibs. Option (3) is cleaner ownership but bundles mockspace-workflow lints into viola's repo, which conflicts with the migration plan's "mockspace owns workflow lints" principle.

Recommendation: option (2) for the built-in pool (one cdylib per pack: `mockspace-builtin-lints`, `mockspace-hilavitkutin-stack-lints`, per-repo lints stay in their repo). The descriptor lists one provider entry per lint. Each lint is one evaluator function. This is the same shape viola already uses for its plugins; the per-lint granularity is at the function level, not the crate level.

## Open questions deferred to subsequent slices

These are load-bearing but do not block the first slice.

**How does `lints.toml` config flow?** The migration plan §"What about LintCfgStore and severity resolution" specifies in-process: lints read `ctx.config.get(lint_name)`. The cdylib boundary passes `(lint_config_bytes, lint_config_len)` as a flat pair. The host marshals the per-lint TOML sub-table into bytes before the call. The cdylib deserialises on its side. This works; the question is the binary encoding (TOML directly? Bincode? A purpose-built schema?). The current `viola-plugin-abi` doesn't pin this beyond "bytes plus length". Subsequent slices settle the encoding choice.

**Vocabulary translation: Document to NamPayload.** Mockspace's `Document` carries (path, source, language, content_hash). Viola's `NamPayload` carries the normalised analysis model produced by the runner role. For lints that operate on raw source (no_todo, file_size, no_bare_pub), the translation is trivial: synthesise a NamPayload with one file entry from the Document. For lints that operate on parsed structure (no_duplicate_fn, undocumented_type, registrable_completeness), the translation needs the runner role to extract the structure first, or the lint embeds its own parser. The migration plan's per-document vs project-scoped split maps onto this: per-document lints often need only the raw source; project-scoped lints often need the parsed structure across files. This question feeds the per-lint port scope and decides whether each lint stages a runner-extract pass first.

**Where does the substrate fit when viola is the runtime?** Task #198 lands a `ViolaEngine` impl of `LintEngine`. The substrate's `MockspaceEngine` placeholder stays for the transitional window. Deletion of `MockspaceEngine` is post-viola-integration and is a separate slice; the first-slice PR does not touch the placeholder.

**Suppression handling.** The substrate's `SuppressionMap` honours `// lint:allow(...)` inline comments and `mockspace.toml`-driven suppressions. With lints running as cdylibs, the suppression filtering happens host-side after the dispatch, identical to the in-process pattern. The substrate-side filter is engine-agnostic; `ViolaEngine` applies it the same way `MockspaceEngine` does. No new suppression vocabulary needed.

## What stays unchanged from the migration plan

The migration plan's destination categorisation (16 + 17 + 8 + 3) is unaffected. The v1 to v2 trait migration cookbook (Patterns A through D) still applies, with the additional mechanical translation step at the FFI boundary the cdylib port adds. Lint names, TOML config keys, severity vocabulary, `lint:allow` inline syntax, `mockspace.toml [lints.<name>]` block shape, agent rules surface: all stable across the cdylib pivot.

## What changes from the migration plan

Lint authoring traits (`Lint`, `PerDocumentLint`) become an engine-internal convenience for the placeholder `MockspaceEngine`. They are not the only path; cdylib lints implement an evaluator function directly against the `LintEvaluateVtable` shape. The migration plan implicitly assumed every lint must impl `Lint`; the post-#609 reality is that some lints live as plain `extern "C"` functions inside cdylibs.

Lint distribution shifts from "Rust dependency in `[lint-crates]`" to "cdylib path resolved by the extension host". Mockspace's existing `[lint-crates]` config is the v1 surface; the cdylib boundary is the v2 surface. Both can coexist for a transitional window: `[lint-crates]` keeps working under `MockspaceEngine`, and a new `[plugins]` section keys cdylib paths the `ViolaEngine` loads.

## Recommendation summary

Lock in Path A. The next slice opens a topic + DOC CL in mockspace specifying:

- The `ViolaEngine` impl boundary (which crate owns it, which substrate types it consumes, what its `LintEngine::run` body does).
- The per-pack cdylib shape (option 2 above: one cdylib per pack, N provider entries per cdylib).
- The first-port lint (no_todo) with end-to-end shape: descriptor, vtable static, evaluator, test harness.
- The config-bytes encoding choice.

After that DOC CL locks, the first src CL ports no_todo, validates the end-to-end shape against the test harness, and lands as the pattern reference. Subsequent slices port the rest of the catalog against that reference.

## See also

- `hilavitkutin/mock/research/202605232100_workunit-cdylib-boundary.md` (the #609 boundary recommendation, viola-tilted).
- `mockspace/mock/research/202605201500_lint-catalog-migration-plan.md` (the v1 to v2 migration plan, pre-#609).
- `mockspace/mock/research/202605201400_viola-engine-integration-shape.md` (the viola-integration shape note the migration plan referenced).
- `viola/mock/crates/viola-plugin-abi/src/vtable.rs:102` (the landed `LintEvaluateVtable`).
- Task #198 (integrate viola as mockspace's primary lint runtime).
- Task #200 (migrate lint config from `mockspace.toml` to `viola.toml`).
- Task #610 (this memo's parent task).
