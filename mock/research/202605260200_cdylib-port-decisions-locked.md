# Cdylib port decisions: locked

**Date:** 2026-05-25
**Status:** Decision record. Op-confirmed the three open design calls from PRs #213, #215, #216 on 2026-05-25. This memo records the picked options and traces forward to the implementation slices each unblocks.
**Scope:** Task #610. After this memo, the catalogue port has zero remaining design unknowns; subsequent rounds are implementation-only.
**Source artefacts (the surveys these decisions resolve):**
- `mock/research/202605252000_cdylib-buffer-ownership-design.md` (PR #213): buffer ownership A vs B.
- `mock/research/202605260000_ast-across-nam-design.md` (PR #215): AST-across-NAM (a) / (b) / (c).
- `mock/research/202605260100_nam-project-scope-design.md` (PR #216): project-scope dispatch (i) / (ii) / (iii).

## The three decisions

### Decision 1: buffer ownership (Option B)

**Picked**: host owns the output buffer; vtable signature carries `*mut Diagnostic` + capacity + out_len. Plugin is a pure function over inputs.

**Concretely**:
- `LintEvaluateVtable` becomes:
  ```rust
  #[repr(C)]
  pub struct LintEvaluateVtable {
      pub evaluate: unsafe extern "C" fn(
          host_ctx: *mut c_void,
          nam: *const NamPayload,
          lint_config_bytes: *const u8,
          lint_config_len: arvo::USize,
          out_entries: *mut Diagnostic,
          out_capacity: arvo::USize,
          out_len: *mut arvo::USize,
      ) -> AbiStatus,
  }
  ```
- Provider id renames to `viola.lint.evaluate.v2` (no-shims-pre-1.0 rule deletes v1 outright).
- Default host-side buffer capacity stays at 256 slots per locked DOC CL R2.
- `DiagnosticBatch` `#[repr(C)]` carrier in `diagnostic.rs` deletes alongside v1 (no consumer references it outside the vtable).

**What this unblocks**: wave-1 cdylib implementation (no-todo port). Two slices, in order:
1. **Viola-side** ABI v2 PR: vtable signature change + `PROVIDER_LINT_EVALUATE` renaming to `.v2` constant + `DiagnosticBatch` deletion. Lands on viola dev.
2. **Mockspace-side** cdylib PR: `mockspace-builtin-lints` crate against v2 vtable. No static-mut buffer; evaluator writes directly into the host-provided slice.

### Decision 2: AST across NAM (Option b)

**Picked**: NAM ships a serialised flat node array. `NamFileEntry` gains a `nodes: *const NamNode` slice; cdylibs walk indices through a `#[repr(C)]` slice without linking tree-sitter.

**Concretely**:
- NAM schema version bump: `V1_0_0 -> V1_1_0`.
- `NamFileEntry` gains a `nodes: BytesRef`-style carrier pointing at a `NamNode` array (the underlying type's pointer + length).
- `NamNode` `#[repr(C)]` shape (starting point; refined at first-port sketch):
  ```rust
  #[repr(C)]
  pub struct NamNode {
      pub kind: arvo::USize,        // index into canonical id table
      pub parent: arvo::USize,      // index into the slice; root = SLICE_LEN
      pub first_child: arvo::USize, // SLICE_LEN if leaf
      pub start_byte: arvo::USize,
      pub end_byte: arvo::USize,
      pub start_row: arvo::USize,
      pub end_row: arvo::USize,
  }
  ```
- Canonical node-kind id table ships at viola-plugin-abi (workspace-canonical, not per-pack). Initial coverage: **Rust mandatory**; Markdown desirable; other languages defer to schema sub-versions.
- Host pre-walks the tree-sitter tree once per parse, serialises into the flat array, exposes via NAM v1.1.0.
- Cdylibs do **not** depend on tree-sitter. Walking is index arithmetic on the host-owned slice.

**What this unblocks**: wave-4 implementation (bucket-2 tree-sitter lints: `no_todo` tree-sitter variant, `export_count`, `no_empty_crate`). Two slices, in order:
1. **Viola-side** NAM v1.1.0 PR: `NamVersion::V1_1_0` const + `NamFileEntry.nodes` field + `NamNode` struct + canonical Rust node-kind id table + host serialiser + `nam_file_nodes(&NamFileEntry) -> Option<&[NamNode]>` accessor.
2. **Mockspace-side** bucket-2 ports PR(s): three cdylibs (or three providers in one cdylib) walking the flat slice.

### Decision 3: project-scope dispatch (Option ii)

**Picked**: two-phase index-then-evaluate. Two distinct vtable slots per project-scoped lint. Phase 1 builds an index; phase 2 reads the index per-file.

**Why (ii) is the right shape** (the agent's PR #216 recommended (i) on catalogue-size grounds; that recommendation was made on weak ABI-design reasoning and is retired):

- **Overflow handling becomes per-file rather than per-project.** A project with 500 cross-crate findings fills the 256-slot DiagnosticBatch immediately under (i); the host has no recovery path because the index lives inside the single call and cannot be replayed. Under (ii), each `evaluate_phase` call writes one file's findings; overflow on one file does not kill the run.
- **Parallelisation surface.** Under (ii) the host parallelises `evaluate_phase` across files (each reads the shared index, writes its own buffer). Under (i) the whole project-scoped lint serialises inside one cdylib call.
- **The index is the natural cache key for future incremental linting.** If lint results ever cache across runs keyed on project content hash, (ii) already factored the cache key (the index) out. (i) bakes it inside the call.
- **The "richer ABI surface" framing was wrong.** The actual delta is one extra `pub unsafe extern "C" fn` slot per project-scoped lint vtable. Cdylib authors implement both phases because the cross-crate algorithm naturally has both.

**Concretely**:
- New vtable shape `LintEvaluateProjectIndexVtable`:
  ```rust
  #[repr(C)]
  pub struct LintEvaluateProjectIndexVtable {
      pub index_phase: unsafe extern "C" fn(
          host_ctx: *mut c_void,
          nam: *const NamPayload,
          lint_config_bytes: *const u8,
          lint_config_len: arvo::USize,
          out_index: *mut IndexBatch,
      ) -> AbiStatus,
      pub evaluate_phase: unsafe extern "C" fn(
          host_ctx: *mut c_void,
          nam: *const NamPayload,
          file_idx: arvo::USize,
          index: *const IndexBatch,
          out_entries: *mut Diagnostic,
          out_capacity: arvo::USize,
          out_len: *mut arvo::USize,
      ) -> AbiStatus,
  }
  ```
- `IndexBatch` `#[repr(C)]` carrier (shape opaque to the host; the host carries it through phase 2 calls). Buffer ownership for `IndexBatch` follows Decision 1's rule: host-allocated; plugin writes through.
- Distinct ProviderId `viola.lint.evaluate-project.v1` (recommended in the memo) for the dispatch entry.
- The host's dispatch loop: for each project-scoped provider, call `index_phase` once with the full NAM, then call `evaluate_phase` per file passing the same index back.
- Output buffer overflow handling reuses Decision 1 / DOC CL R2: `AbiStatus::Internal` on capacity exceeded, `*out_len` set to the count the lint would have emitted.

**`IndexBatch` contract** (settled here, not deferred to wave-5 SRC CL):

- **Ownership**: host pre-allocates the `IndexBatch` buffer; plugin writes through during `index_phase`. Same rule as Decision 1's diagnostic buffer. Host frees after the last `evaluate_phase` call for the project. Buffer is alive across both phases for one project run.
- **Capacity model**: host pre-allocates with a default cap of `MAX_INDEX_ENTRIES = 1 << 20` (one million entries; enough for `no_duplicate_fn` and `undocumented_type` on any realistic project, sized in advance from project file count). If `index_phase` overflows, plugin returns `AbiStatus::Internal` and writes the needed capacity into a sibling `*mut arvo::USize` field; host re-allocates and retries. The default cap may shrink in future workload data; the negotiation path stays.
- **Opacity contract**: `IndexBatch.entries` is `*const c_void` to the host. The host never inspects content; it shuttles the same pointer from `index_phase` output to `evaluate_phase` input. Each cdylib defines its own internal layout. Cross-cdylib index sharing is explicitly out of scope; if a future workload pressures it, that ships as a separate ABI extension.

**What this unblocks**: wave-5 implementation (bucket-3 cross-crate lints: `no_duplicate_fn`, `undocumented_type`). Three slices, in order:
1. **Viola-side** project-scope ABI PR: `LintEvaluateProjectIndexVtable` + `IndexBatch` carrier (with the contract above) + `PROVIDER_LINT_EVALUATE_PROJECT` const + host-side dispatch helpers including the re-allocate-on-overflow loop.
2. **Mockspace-side** bucket-3 ports PR(s): two cdylibs (or two providers in one cdylib) splitting the existing `CrossCrateLint::check_all` body into `index_phase` (build name-to-site map) and `evaluate_phase` (per-file diagnostic emission).
3. **Future optimisation** (out of scope here): index caching across runs when project hash unchanged; deferred until a real workload pressures it.

## Implementation order

The three viola-side ABI slices can land independently (no cross-decision dependencies). Suggested order for review-load reasons:

1. **Viola PR ABI v2 (Decision 1)**: smallest change, single vtable signature. Unblocks wave 1, 2, 3 all at once.
2. **Mockspace cdylib for no-todo against v2 (Decision 1)**: pattern reference. Other wave-1/2/3 ports follow this shape.
3. **Mockspace bucket-1 ports against v2 (Decision 1)**: file-size, actionable-errors, no-bare-pub. Mechanical follow-on against the wave-1 v2 vtable; no further ABI surface change.
4. **Viola PR NAM v1.1.0 (Decision 2)**: schema bump + node-kind table. Unblocks wave 4.
5. **Mockspace bucket-2 cdylibs (Decision 2)**: three ports against v1.1.0.
6. **Viola PR project-scope ABI (Decision 3)**: two-phase vtable + IndexBatch. Unblocks wave 5.
7. **Mockspace bucket-3 cdylibs (Decision 3)**: two ports splitting into index + evaluate phases.

## What this memo does NOT lock

Each decision has follow-up axes that surface during implementation:

- **Decision 1**: NAM payload-side wire shape for per-lint config bytes (TOML serialised? bincode? per-lint discriminant?). Defer to first-port SRC CL.
- **Decision 2**: extending the canonical id table for non-Rust grammars (Markdown, JSON, TOML, etc.) ships per-language sub-version. Each grammar addition is its own design call.
- **Decision 2**: `NamNode.kind`'s exact width (the agent's recommendation per PR #215's reviewer note was "arvo numeric, exact width TBD at sketch time"; settled at the wave-4 first-port sketch).
- **Decision 3**: `IndexBatch` `#[repr(C)]` outer carrier field layout (the `*const c_void` plus length plus capacity-needed slot for the overflow path). The ownership, capacity model, and opacity contract are pinned above; the exact field ordering and any padding ships in the viola-side ABI PR.

## Sub-tasks to file under #610

The implementation order names seven concrete slices. Workspace tasks for each are created alongside this memo so progress is trackable per slice:

1. #610.A: viola: ABI v2 vtable (lint evaluate, host-owned buffer)
2. #610.B: mockspace: no-todo cdylib against v2 (wave 1 first port)
3. #610.C: mockspace: bucket-1 ports (file-size, actionable-errors, no-bare-pub) against v2
4. #610.D: viola: NAM v1.1.0 schema (flat node array, canonical Rust id table)
5. #610.E: mockspace: bucket-2 ports (no-todo tree-sitter variant, export-count, no-empty-crate) against v1.1.0
6. #610.F: viola: two-phase project-scope ABI (LintEvaluateProjectIndexVtable, IndexBatch)
7. #610.G: mockspace: bucket-3 ports (no-duplicate-fn, undocumented-type) split into index + evaluate phases

Each becomes a workspace task; this memo is the spec they reference.

## See also

- `mock/research/202605252000_cdylib-buffer-ownership-design.md` (PR #213): option survey that produced Decision 1.
- `mock/research/202605260000_ast-across-nam-design.md` (PR #215): option survey for Decision 2.
- `mock/research/202605260100_nam-project-scope-design.md` (PR #216): option survey for Decision 3.
- `mock/research/202605252300_lint-port-priority-second-wave.md` (PR #214): port-priority memo grouping lints into five waves.
- `mock/research/202605231400_lint-cdylib-vs-workunit-boundary.md`: why the cdylib boundary is the right shape.
- `mock/design_rounds/202605251600/202605251600_changelist.doc.lock.md`: the locked DOC CL covering R0 through R5 + first-port no-todo scope.
- Workspace tasks #610 (parent), #254 (viola becomes a hilavitkutin app; eventual WorkUnit reshape).
