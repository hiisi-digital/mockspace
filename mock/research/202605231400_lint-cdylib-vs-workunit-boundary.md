# Lint cdylib vs WorkUnit boundary

**Date:** 2026-05-23
**Status:** Research memo capturing a design conversation that walked an in-flight PR series back to its root design question. Task #607 reframed and re-blocked off this memo.
**Scope:** Why the in-flight `mockspace-lint-abi` PR (#187) was closed unmerged; what the right shape actually is for mockspace's lint dispatch boundary; what blockers stand between today and that shape; what mockspace can ship in the meantime.
**Source artifacts:**
- mockspace PR #186 (workspace pattern memo for contracts-first with temporary backing; merged)
- mockspace PR #187 (the bespoke cdylib ABI iteration; closed unmerged)
- `mock/research/202605211200_lint-schema-design.md` (the schema lock for mockspace's lint catalog + Lint trait + dispatch modes)
- `mock/research/202605202200_lint-primitive-consolidation.md` (the consolidation proposal the schema memo locks)
- `mock/research/202605201700_engine-preprocessor-architecture.md` (engine + preprocessor design)
- `mock/research/202605201400_viola-engine-integration-shape.md` (viola engine integration)
- viola `docs/PLUGIN-ABI-V1-DESIGN.md` (viola's plugin ABI v1, where the `evaluate(NAM) -> Diagnostics` shape lives)
- hilavitkutin `mock/crates/hilavitkutin-extensions/` (the descriptor + provider + ABI version gating layer)
- hilavitkutin `mock/crates/hilavitkutin-linking/` (the pull-based dlopen primitive)
- workspace task #254 (the viola-becomes-a-hilavitkutin-app conversion: scheduler + WorkUnits, state in Resources / Columns)
- workspace memory `feedback_hilavitkutin_workunit_mental_model.md` (hilavitkutin apps = WorkUnits + scheduler data)
- workspace memory `feedback_contracts_first_with_temp_backing.md` (the contracts-first pattern)

## What set this off

Task #607 asked for the consumer-extension surface for mockspace lints. The opening framing offered three paths (Path A viola plugin scaffolding, Path B `Box<dyn Lint>` interim, Path C deferral); the user chose a contracts-first hybrid: write the trait surface today per the eventual spec, ship a temporary backing, swap the backing when the spec is ready. Standard workspace pattern. The follow-up PR series was supposed to ship that.

What actually happened: each turn of the PR refinement surfaced that the previous shape was wrong. The drift went:

1. **First shape:** document the existing `pub trait Lint` + `inventory::submit!{CatalogEntry { ... }}` as the consumer-extension story. User correction: cdylibs strip the Rust sidecar machinery; inventory does not reach across the cdylib boundary. The "in-process registration via inventory" path is for static-linked Rust consumers ONLY, not for cdylibs.

2. **Second shape:** bespoke cdylib ABI per the bench-harness three-symbol precedent (abi_hash + metadata + evaluate_vtable). Add the lint vtable shape, ship a libloading-based host loader as the temporary backing. User correction: dispatch should be single-path. Builtins should not be a special-cased in-process path while consumers go through the cdylib boundary; that splits the engine and lets the cdylib path rot independently. Drop inventory entirely; all lints (builtin and consumer) go through the same cdylib ABI.

3. **Third shape:** multi-lint-per-cdylib bespoke descriptor + ABI hash. The cdylib hosts many lints; the descriptor is a header + entry array. User correction: the workspace already ships `hilavitkutin-extensions` for this exact purpose. Don't reinvent the descriptor protocol; consume `hilavitkutin-extensions` like viola does.

4. **Fourth shape:** depend on `hilavitkutin-extensions`, expose lint-domain vtable, lint-domain wire types. User correction: viola's plugin ABI v1 shape (`evaluate(NAM) -> Diagnostics`) is wrong; viola is supposed to be a hilavitkutin app per task #254, which would express lints as `WorkUnit`s with declared `AccessSet` over engine columns. Viola's current `evaluate(whole NAM)` plugin function is a leftover from before viola was supposed to consume the engine. Mockspace should NOT inherit that shape.

The drift was real. Each correction sharpened the design target. The final correction surfaces a load-bearing design question the prior shapes were dodging: what IS a lint at the workspace level, and how does it cross the cdylib boundary?

## The actual right model

The workspace's mental model for "what does work in a mockspace consumer crate look like" is unambiguous when stated plainly:

- The host crate (mockspace, viola, future tools) is a hilavitkutin app.
- Work units of the host (lint primitive impls, render pipeline stages, validation passes, parsers, walkers, finding aggregation, output formatters) are `WorkUnit`s with declared `Read: AccessSet` and `Write: AccessSet` over engine columns + resources.
- The hilavitkutin scheduler plans phases, trunks, fibers across all loaded WorkUnits, dispatches morsels per core.
- Consumer-authored extensions ship cdylibs that contribute additional WorkUnits to the scheduler.
- A cdylib's contribution is a static catalog of WorkUnit descriptors, each carrying the AccessSet declaration + a vtable pointer to the execute body.

Applied to lints specifically:

- A lint is a WorkUnit (or a small bundle of related WorkUnits).
- Its `Read: AccessSet` declares which engine columns it consumes: `Column<SynAstNode>`, `Column<TokenSpan>`, `Column<TreeSitterTree>`, `Column<MarkdownNode>`, etc.
- Its `Write: AccessSet` declares what it produces: `Column<Finding>`, or per-finding-kind columns.
- Its execute body runs on a morsel of the input column(s) and writes findings via the access set.

The dispatch model that falls out of this is exactly what `mock/research/202605211200_lint-schema-design.md` describes (engine drives, pre-warms caches, calls per-document or per-project), but expressed as engine-level WorkUnit scheduling instead of an ad-hoc `Lint` trait the engine calls in a loop. Per-document dispatch is a WorkUnit reading `Column<Document>`. Project-scoped dispatch is a WorkUnit reading `Resource<Project>`. Two-phase is two WorkUnits sharing intermediate columns. Cache pre-warming is the engine running its parser WorkUnit in an earlier phase before the lint WorkUnits enter the dispatch.

The CatalogEntry shape mockspace already designed maps onto this:

| Mockspace CatalogEntry field | WorkUnit equivalent |
|---|---|
| `name` / `description` | WorkUnit metadata |
| `kind` (open-string discriminator) | Not needed; the WorkUnit's type IS its discrimination |
| `mode` (PerDocument / ProjectScoped / TwoPhaseProject) | The declared AccessSet (Column vs Resource vs Column + intermediate) |
| `needs_syn_ast` / `needs_tree_sitter` | Implied by the AccessSet membership |
| `staging_aware` / `editor_skip` | Per-run config affecting which WorkUnits get added to the schedule |
| `default_severity` / per-gate overrides | Per-WorkUnit config the engine consumes when dispatching |
| `finding_kinds` | The output column's typed discriminant |
| `instantiate: fn(config, scope) -> Box<dyn Lint>` | A constructor that builds the WorkUnit instance with its config |

The `Lint` trait disappears as the canonical contract. The CatalogEntry-as-descriptor is the right name; the trait was the sugar for Rust-side `impl Lint for MyType`. With the WorkUnit framing, the descriptor pairs a static metadata block with a vtable that exposes the WorkUnit's execute body (and any associated lifecycle hooks).

## Why mockspace cannot ship this today

The WorkUnit-cdylib boundary is upstream-unsolved. Three concrete pieces have to land before mockspace's lint ABI can be authored cleanly:

1. **Hilavitkutin runtime is in flight.** The engine megaround #334-#345 (Scheduler::run signature, ResourceSnapshot, per-core dispatch, DependencyGraph CSR, AdaptMetrics, the plan/dispatch/adapt/thread/morsel/resource passes) is unfinished. WorkUnit dispatch as a runtime concept is not yet shipped.

2. **The WorkUnit-cdylib boundary itself is undesigned.** Hilavitkutin's plugin host layer (`hilavitkutin-linking` + `hilavitkutin-extensions` + `hilavitkutin-extensions-macros`) provides the cdylib descriptor + provider protocol, but does not currently carry a WorkUnit-shaped vtable type. A new vtable shape (`WorkUnitVtable`? `KitEntry`?) needs to be designed that crosses the cdylib boundary: declared AccessSet, declared inputs/outputs, execute function pointer, lifecycle hooks. This is a hilavitkutin-level extension, not a mockspace-level one.

3. **Viola hasn't been turned into a hilavitkutin app yet.** Task #254 captures this. Until viola actually consumes the engine, viola's plugin ABI v1 stays as the `evaluate(NAM) -> Diagnostics` placeholder. Mockspace cannot lean on viola's shape because that shape is a known-temporary leftover.

The honest cost picture: implementing the WorkUnit-cdylib boundary now is multi-week work spanning at least hilavitkutin (define the WorkUnit vtable + lifecycle), hilavitkutin-extensions-macros (extend `#[export_extension]` to emit WorkUnit catalog entries), and a sample consumer to validate the boundary. Mockspace's lint surface is downstream of that and cannot be designed sensibly in advance of it.

## What mockspace can ship in the meantime

Three things that do NOT depend on the WorkUnit-cdylib boundary:

1. **Drop the `inventory` dep** from mockspace-rs. Builtins move to an explicit constructor list inside the engine crate. Static composition, no link-magic. Aligns with hilavitkutin's "static composition is the dispatch" rule and removes a dependency the workspace has explicit guidance against (no `inventory`, no `#[ctor]`, no `.init_array` registration). Consumer-extension via this path becomes unavailable, which is fine because consumer-extension is parked anyway.

2. **Continue the existing lint dispatch shape** for built-ins. The `Lint` trait stays as Rust-side sugar; the engine calls per-document / per-project per the catalog mode. Performance and ergonomics stay where they are; nothing is broken. When the WorkUnit-cdylib boundary lands, this in-process dispatch gets re-expressed as in-process WorkUnit dispatch (mockspace becomes a hilavitkutin app the same way viola will, per #254-shaped follow-up for mockspace).

3. **Document the WorkUnit-shaped intent in the catalog memo.** A short addendum to `mock/research/202605211200_lint-schema-design.md` (or a sibling research memo) names the eventual reframing: today's `pub trait Lint` is the in-process precursor to a WorkUnit-typed lint that the eventual hilavitkutin-app version of mockspace will host. Captures the migration target so future readers don't redo this conversation.

The contracts-first pattern still applies: contracts are written against the eventual shape, with a temporary backing. The catch is that the EVENTUAL CONTRACT (`WorkUnit` with `AccessSet`) is itself not yet authored at the workspace level. Until it is, mockspace cannot author the trait surface against it. The contracts-first pattern presupposes the eventual spec exists; here, it does not.

## Where #607 lands

Task #607 is reframed from "ship the consumer-extension surface" to "block on WorkUnit-cdylib boundary." The implementation work that was in the original task description (Path A / Path B / Path C decision, the cdylib ABI design, the loader) gets transferred to two new tasks:

- A hilavitkutin-side task: design the WorkUnit-cdylib boundary (the vtable shape + lifecycle hooks + AccessSet-across-FFI strategy). This is the load-bearing piece and lives upstream of mockspace.
- A mockspace-side task: once the WorkUnit-cdylib boundary lands, port mockspace's lint catalog onto it. This is downstream and structurally mechanical once the boundary exists.

Both new tasks are blocked by the hilavitkutin runtime megaround (#334-#345) and by viola's hilavitkutin-app conversion (#254). The blocker chain is real and concrete; the in-flight rush to ship a stepping-stone ABI was the wrong shape.

## What this memo does NOT lock

- The detailed shape of the WorkUnit-cdylib boundary itself. That's the hilavitkutin-side design task this memo creates a blocker reference for, not a thing this memo answers.
- Whether mockspace ever needs a separate `mockspace-lint-abi` crate, or whether the lint domain just emits WorkUnit-catalog entries directly via hilavitkutin-extensions-macros. The answer falls out of the WorkUnit-cdylib boundary design.
- Whether mockspace should also become a hilavitkutin app proactively (parallel to viola task #254). That's a strategic call. Plausibly yes, since the lint dispatch shape inside mockspace's engine ends up being WorkUnit-shaped anyway; once the engine is hilavitkutin, the cdylib boundary lands for free with whatever hilavitkutin-extensions ships. But this memo doesn't decide it.
- Whether the current `pub trait Lint` should be removed now (alongside the `inventory` drop in the meantime-shippable list) or kept as Rust-side sugar until #610 lands. The "meantime" framing earlier in this memo treats the trait as staying; the eventual reframing dissolves the trait into WorkUnit-shaped impls. The scope decision (drop now vs port later) is up to #610's planning; the memo leaves it open because either order produces the same end state.

## Pattern lesson

The contracts-first-with-temporary-backing pattern only works when the eventual contract exists somewhere. When the eventual contract is itself unshipped (the case here: WorkUnit-cdylib boundary is upstream and unsolved), the temporary backing has nothing to anchor to; the trait surface keeps drifting because the target keeps clarifying. The correct move in that situation is NOT to ship the stepping stone; it is to admit the target needs to be designed first, file the blockers, and not pretend the contract is stable when it is not.

This memo is the record of figuring that out the hard way (four refinement turns on a single PR before the actual root issue surfaced). For future agents reading: when the design conversation keeps revealing that you're not building toward the right shape, the right move is to stop and surface the blocker, not to redesign one more time.
