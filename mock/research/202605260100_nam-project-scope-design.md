# Design: NAM project scope across the cdylib boundary (wave 5 opener)

**Date:** 2026-05-25
**Status:** Pre-implementation design memo. Resolves the load-bearing question for wave 5 of the lint catalog cdylib port: how does a cross-document lint (one that aggregates state across a project's files) get its dispatch shape inside a cdylib?
**Scope:** Task #610 wave 5 (per the port-priority memo at `mock/research/202605252300_lint-port-priority-second-wave.md`). Decides which option carries bucket-3 lints (`undocumented_type`, `no_duplicate_fn`) across the cdylib boundary.
**Source artefacts:**
- `mock/research/202605252300_lint-port-priority-second-wave.md` (port-priority memo; flags this as the wave-5 opener).
- `mock/research/202605260000_ast-across-nam-design.md` (PR #215; the wave-4 sibling design call this memo mirrors in shape).
- `mock/research/202605252000_cdylib-buffer-ownership-design.md` (PR #213, open at op): the v1 buffer-ownership question; this memo's answer is orthogonal.
- `viola/mock/crates/viola-plugin-abi/src/nam.rs` (NAM v1.0.0 schema; per-file entries iterable as a slice).
- `viola/mock/crates/viola-plugin-abi/src/vtable.rs` (the `LintEvaluateVtable` shape).
- `lint-rules/src/no_duplicate_fn.rs:33-92` (in-process `check_all(&[(&str, &LintContext)])` reference impl walking all crates).
- `lint-rules/src/undocumented_type.rs:31-130` (in-process `check_all` impl scanning definitions across crates and external references).

## The question

Wave 5 ports cross-crate lints to the cdylib boundary. The bucket-3 lints maintain aggregate state across all of a project's documents: `no_duplicate_fn` builds a function-name to definition-site map then flags collisions; `undocumented_type` tracks `pub` type definitions across crates and flags those used externally but missing rustdoc. Both lints' in-process API takes `&[(&str, &LintContext)]` (all crates plus their parse contexts) and returns findings.

NAM v1.0.0 already carries per-file entries as a contiguous slice. So *iteration* across a project's files is technically available: a cdylib's `evaluate` reads the slice, walks each entry, builds whatever aggregate state it needs. The unresolved question is what the *dispatch shape* looks like: when does the host call the cdylib, with what input, and how does the lint maintain state between calls (or does it)?

## Three options

### Option (i): Single-dispatch model (recommended)

The cdylib registers as project-scoped via a descriptor flag (or a distinct ProviderId variant such as `viola.lint.evaluate-project.v1`). The host calls `evaluate` exactly once per project run. The NAM payload contains all project files as the existing `NamFileEntry` slice. The cdylib iterates the slice, builds its own aggregate state inside `evaluate`, emits all findings into one `DiagnosticBatch`, returns.

**What this requires**:
- A descriptor flag (or distinct ProviderId) signalling "project-scoped" dispatch. The flag changes the host's dispatch loop from one-call-per-file to one-call-per-project.
- The `DiagnosticBatch` 256-slot fixed cap (locked DOC CL R2) and overflow status apply as before. Cross-crate lints may emit more than 256 findings on a project with many collisions; overflow handling reuses the same `AbiStatus::Internal` path. A future enhancement may carry batched-pagination if real workloads pressure it.
- No NAM schema change. v1.0.0 already covers project iteration.

**Cost**: cdylib body is mechanically the same as the in-process `check_all(&[(&str, &LintContext)])` impl: walk all files, build dedup state, emit findings. The translation is one accessor call from NAM payload to file slice. Dispatch frequency drops to once per run rather than once per file.

**Risk**: per-lint dedup state inside the cdylib means multiple project-scoped lints building similar indexes (e.g. both `no_duplicate_fn` and `undocumented_type` walk every pub item) duplicate work. At the catalogue's eventual scale (16 mockspace built-ins plus 17 stack-lints, of which only 2-5 are bucket-3), the duplication is bounded and acceptable. Sharing indexes across lints is a future optimisation (see option iii); not a v1 requirement.

### Option (ii): Two-phase index-then-evaluate model

The cdylib exports two vtable slots (or two distinct provider entries). Phase 1: host calls `index_phase(NAM) -> IndexBatch` once per project, lint walks all files and emits an opaque index. Phase 2: host calls `evaluate_phase(NAM, file_idx, IndexBatch) -> DiagnosticBatch` per file, lint reads its own index to detect collisions involving the named file.

**What this requires**:
- A new `LintEvaluateProjectIndexVtable` (or extension to the existing vtable). Two function pointers per project-scoped lint.
- An `IndexBatch` wire shape (`#[repr(C)]`) for the index payload returned from phase 1 and consumed in phase 2. The shape is opaque to the host; the host carries it through to phase 2 calls.
- A descriptor flag signalling "use two-phase dispatch" so the host knows to call `index_phase` before `evaluate_phase`.

**Cost**: dispatch maps to existing per-document infrastructure for phase 2 (the same per-file loop). Index built once per project, reused across files. Two trip points (host to cdylib to host to cdylib per project plus per file) at the price of a richer ABI surface.

**Risk**: the `IndexBatch` shape is the new design surface. Either it is opaque (the host hands it back unchanged to phase 2) at the cost of per-lint allocation in the host's memory (or static-mut in cdylib, matching the buffer-ownership memo's framing), or it is structured at the cost of locking the index format into the ABI. Both have costs that compound across catalogue size. Adds two function-pointer slots per project-scoped lint to the provider descriptor.

### Option (iii): Host-provided canonical indexes

The host pre-computes shared indexes (e.g., a `NamItemRegistry` carrying every pub item with file location and rustdoc presence) and exposes them inside the NAM payload as additional carrier fields. Cross-crate lints read from the host-provided index rather than building their own. NAM gains a versioned `indexes: *const c_void` slot pointing at a tagged-union of canonical index shapes.

**What this requires**:
- A canonical index catalog at viola-plugin-abi (or per-language). `NamPubItemIndex`, `NamFnNameIndex`, `NamTypeDefIndex` and similar wire shapes.
- Host-side index builders covering each canonical index, run once per project before lint dispatch.
- A NAM schema bump (v1.0.0 to a future v1.x.0) for the index slot.

**Cost**: shared indexing work amortised across all project-scoped lints. Cdylib body is pure query, no walk-and-aggregate logic. Lowest per-cdylib runtime cost at the price of upfront catalog design.

**Risk**: locks the canonical index shapes into the ABI. Adding a new project-scoped lint that wants a not-yet-canonical index either requires extending the catalog (every release of the schema) or duplicates index work in its own cdylib body (which defeats the option's value). The catalog grows with each new bucket-3 lint kind, becoming a maintenance surface in its own right.

## Recommendation: Option (i)

The single-dispatch model is the right shape for wave 5.

The decisive comparison is between option (i) and option (ii). Both produce correct results; the difference is where the index work lives (cdylib body vs explicit phase 1 export). Option (ii) earns its complexity only when the index is genuinely reusable across multiple `evaluate_phase` calls in ways that meaningfully reduce work, but the per-file dispatch model that pays off the index reuse is itself a v2 optimisation: project-scoped lints currently emit all findings in one walk, and splitting them per-file would require restructuring the lints' core algorithm. Option (ii) is over-engineered for the catalogue size.

Option (iii) trades upfront catalog design for downstream cdylib-body simplicity, but the catalog design itself becomes the load-bearing piece. At only 2-5 bucket-3 lints in the foreseeable workspace, the catalog's coverage either remains narrow (insufficient for new lints) or overshoots (carrying indexes no consumer uses). Option (i) defers this trade until a real workload pressures it; the eventual upgrade path is "introduce host-provided indexes as an optional NAM extension consumers can opt into" rather than ABI-mandated upfront.

Option (i) keeps the cdylib body shape mechanically close to the in-process `CrossCrateLint::check_all` impl, which makes wave 5's port mechanical: read NAM file slice, run the existing logic, emit findings. The DOC CL R2 overflow protocol carries the multi-finding case cleanly. Per-lint dedup duplication is bounded by catalogue size.

**Agent's call**: Option (i). Op confirmation point flagged: the descriptor flag vs distinct ProviderId variant choice is the secondary axis (a flag is cheaper to extend later; a distinct variant is cheaper to filter at host dispatch). Recommendation here is the distinct ProviderId (`viola.lint.evaluate-project.v1`) on dispatch-clarity grounds, mirroring the wave-4 memo's preference for shape clarity over schema thrift.

## What this memo does NOT lock

- The exact descriptor-flag vs distinct-ProviderId choice. The recommendation here is the distinct ProviderId; the wave-5 first-port round can pick the alternative if dispatch-loop ergonomics make the flag form cheaper.
- Whether project-scoped lints share the existing `DiagnosticBatch` slot or get a separate output buffer. The recommendation here is "share the existing slot"; the locked DOC CL R2 cap and overflow protocol apply uniformly. A future workload that hits the cap repeatedly may motivate per-cap negotiation, but not now.
- Whether the host pre-walks the file slice into language-specific groups (Rust files vs Markdown files vs Other) before passing to the cdylib, or leaves the cdylib to filter via `NamFileEntry.language`. The recommendation here is "cdylib filters"; the host stays oblivious to per-lint language scope. The per-lint config-bytes pathway carries any language restriction.
- The fate of the `staging_aware` / `editor_skip` config bits that bucket-3 lints currently honour in-process. These are host-side filtering knobs that decide whether a cross-crate lint dispatches at all on a given run. They stay host-side under option (i); the cdylib does not need to know about them.

## Open questions for op

1. **Option (i), (ii), or (iii)?** (Agent's call: i, on simplicity and catalogue-size grounds.)
2. **If (i): descriptor flag or distinct ProviderId variant?** (Agent's call: distinct ProviderId `viola.lint.evaluate-project.v1` on dispatch-clarity grounds.)
3. **If (i): does `DiagnosticBatch` overflow on project-wide findings require any protocol enhancement?** (Agent's call: no; reuse R2's `AbiStatus::Internal` and revisit if a real workload pressures it.)

## What this memo does NOT do

- Edit `viola-plugin-abi`'s vtable.rs or descriptor types. Any ProviderId addition ships as a viola-side slice once op confirms option (i).
- Implement any bucket-3 lint port. Those ship after the descriptor change.
- Address the wave-4 AST-across-NAM question. That is its own memo (`mock/research/202605260000_ast-across-nam-design.md`), structurally distinct.

## How wave 5 fits the larger arc

Together with the wave-4 memo, this completes the design front for the lint catalog cdylib port:
- Wave 1 (no-todo regex): gated on PR #213.
- Waves 2 and 3 (file-size, actionable-errors, no-bare-pub): mechanical follow-ons against the wave-1 shape, no new design work.
- Wave 4 (AST-shaped bucket): pre-staged at PR #215.
- Wave 5 (cross-crate bucket): pre-staged here.

Once op decides PR #213 (wave 1 gating) and confirms PR #215 + this memo's open questions, the catalogue port becomes purely implementation work with no remaining design unknowns.

## See also

- `mock/research/202605252300_lint-port-priority-second-wave.md` (the port-priority memo flagging this as the wave-5 opener).
- `mock/research/202605260000_ast-across-nam-design.md` (PR #215; the wave-4 sibling).
- `mock/research/202605252000_cdylib-buffer-ownership-design.md` (PR #213; wave-1 gating, orthogonal question).
- `viola/mock/crates/viola-plugin-abi/src/nam.rs` (NAM v1.0.0; the slice this memo's option i walks).
- `lint-rules/src/no_duplicate_fn.rs`, `lint-rules/src/undocumented_type.rs` (bucket-3 reference impls).
- Workspace tasks #610 (parent), #254 (viola becomes a hilavitkutin app; the eventual WorkUnit reshape).
