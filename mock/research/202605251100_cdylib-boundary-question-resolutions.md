# Cdylib boundary: resolutions to the five open questions

**Date:** 2026-05-25
**Scope:** mockspace task #610. Third slice. Proposes concrete resolutions to the five open questions the no-todo cdylib feasibility sketch (slice 2) surfaced.
**Parent memos:** `mock/research/202605251000_lint-catalog-cdylib-boundary.md` (slice 1: Path A recommendation), `mock/research/sketches/202605251030_no-todo-cdylib-shape.md` (slice 2: feasibility sketch with five open questions).
**Status:** proposal memo. The DOC CL slice that follows either accepts these resolutions or op pushes back per question before formal ceremony.

## TLDR

Three of the five open questions are mechanical and have one defensible answer each (host_ctx convention, cdylib-load entry point, batch buffer-full as a later-slice deferral). Two carry real design tradeoffs and the proposal here is the agent's call: NAM schema as minimal-per-file-v1.x (option 1, not the parallel-vtable sidestep option 2), and per-lint provider ids (the sketch's existing shape, not per-pack with internal routing). Both load-bearing answers are flagged for explicit op confirmation; the rest ship as the proposal stands.

## Resolution 1: NAM payload schema (LOAD-BEARING)

**Proposal:** Define a minimal NAM v1.0.0 schema with per-file `path: BytesRef`, `language: u32`, `source: BytesRef` entries. The cdylib evaluator reads these fields through accessor helpers in `viola-plugin-abi`. Lints that need only raw source (no-todo, file-size, no-bare-pub, content-regex family) work against this minimal schema directly. Lints that need parsed structure (no-duplicate-fn, undocumented-type, registrable-completeness) require schema enrichment in a later v1.x minor revision.

**Reasoning:** The alternative (option 2, parallel `viola.lint.evaluate-raw.v1` vtable) fragments the cdylib protocol. Path A's value is one boundary, not two; introducing a second vtable shape erodes that. Option 1 commits to NAM as the universal lint input vocabulary, with schema growth handled by NAM's documented version axis (`NamVersion::major/minor/patch`). The minor-revision growth path matches the file's module doc: "the wire shape of the payload is deferred to a minor revision".

**Tradeoff explicitly accepted:** lints landing in the first port wave only see path + language + source. Cross-file analyses (no-duplicate-fn) wait for the v1.1 schema bump. The migration plan's "stays mockspace built-in" pool includes no-duplicate-fn, so this is a real deferral. But: porting all 16 built-ins at once is not the first slice's goal. Porting no-todo is. Schema enrichment lands when the second-wave lints need it.

**Op confirmation question:** is the minimal-schema-with-growth-path the right pick, or does op prefer the parallel-vtable sidestep? The agent's call is option 1 on protocol-coherence grounds.

## Resolution 2: DiagnosticBatch buffer-full handling (deferred to later slice)

**Proposal:** Default fixed-cap batch (e.g. 256 slots per call). When full, the evaluator returns `AbiStatus::Internal` and emits no further diagnostics for that call. The host treats `Internal` status as a non-fatal warning and continues to the next lint.

**Reasoning:** The buffer-full case is an edge condition for the first-port lints. no-todo's 99% case is a handful of diagnostics per file. Designing a retry-with-fresh-batch protocol or a growable buffer is premature optimisation that complicates the cdylib boundary before any real workload pressures it. The fixed-cap-with-`Internal`-status answer is the simplest behaviour that fails safely.

**Later-slice scope:** when a lint workload hits the cap regularly, the slice that surfaces the pressure proposes the retry protocol (probable shape: cdylib returns `Pending`, host calls again with the same NAM and a fresh batch, cdylib resumes from internal offset). This is the standard chunked-output pattern; lifting it into the protocol when needed is straightforward.

**No op confirmation needed.** The deferral itself is the answer.

## Resolution 3: Per-lint host_ctx convention (mechanical)

**Proposal:** Reuse viola's existing host_ctx shape. The cdylib treats `host_ctx: *mut c_void` as opaque; mockspace lints don't read through it for any mockspace-specific state. Cross-cutting host state (workspace root, run surface) is reachable through the NAM payload's `run_context` block (per `viola-plugin-abi/src/nam.rs` module doc reference to NAM §9.3).

**Reasoning:** Forking host_ctx between viola lints and mockspace lints would be a second boundary divergence after the protocol unification work just committed. Mockspace's workflow-specific lints (changelist-doc-gate, design-doc-source-mismatch) read mockspace artefacts via file paths, which the NAM payload's per-file entries already expose. No new host_ctx shape needed.

**No op confirmation needed.** The reuse is the natural shape.

## Resolution 4: Per-lint vs per-pack provider id convention (LOAD-BEARING)

**Proposal:** Per-lint provider ids. Each lint gets a distinct `<pack>.lint.<name>.v1` id; one cdylib hosts N entries in its descriptor `providers` table. The sketch's `mockspace.lint.no-todo.v1` shape generalises to `mockspace-builtin.lint.<name>.v1` for the built-in pool, `stack.lint.<name>.v1` for the shared pack, `<repo>.lint.<name>.v1` for repo-local.

**Reasoning:** Three considerations push toward per-lint over per-pack.

First, descriptor enumeration. The host walks `ProviderEntry[]` once at load time. With per-lint ids the host learns each lint's existence from the descriptor directly. With per-pack the host invokes one provider with a lint-name parameter; the host has no way to know which lints the pack ships until it asks each, which is awkward against the descriptor's static shape.

Second, dispatch surface. Per-lint gives the host a stable function pointer per lint; LLVM may de-virtualise or inline through it. Per-pack routes through an internal switch in the cdylib's single evaluator, which the host cannot see through.

Third, debugging and observability. A backtrace or profile that names `mockspace-builtin.lint.no-todo.v1` is more legible than one that names the pack id with an opaque internal selector.

**Tradeoff explicitly accepted:** N entries per descriptor means the descriptor's `providers` table grows linearly with lint count. For the built-in pool (16 lints) and stack pool (17 lints) this is a modest table. The `MAX_DESCRIPTOR_LIST_LEN` constant in `hilavitkutin-extensions` caps the descriptor list size; the cap covers the per-lint shape without strain.

**Op confirmation question:** is per-lint the right pick, or does op prefer per-pack with internal routing? The agent's call is per-lint on enumeration + dispatch + observability grounds.

## Resolution 5: Cdylib-load entry point (mechanical given locked sequence)

**Proposal:** During the transitional window, mockspace reads cdylib paths from a new `[plugins]` section in `mockspace.toml`. After viola integration lands (task #198), the config migrates to `viola.toml` per task #200. The `ViolaEngine` impl reads whichever config is active at the consumer repo.

**Reasoning:** The locked sequence already commits to `viola.toml` as the eventual home (#200). The transitional window keeps `mockspace.toml [plugins]` for repos that adopt the cdylib boundary before viola integration is finished. After viola integration the section relocates; consumer repos see one migration step.

The `[plugins]` section is additive to existing `mockspace.toml`; the `[lint-crates]` section coexists during the transitional window and is honoured by the `MockspaceEngine` placeholder. When viola integration lands, both sections deprecate together in favour of `viola.toml`'s plugin config.

**No op confirmation needed.** The locked sequence direction settles this.

## What ships when these resolutions accept

The next slice opens a topic + DOC CL in mockspace that commits the five resolutions as design state. The DOC CL specifies:

1. Path A directional commitment (parent memo's recommendation).
2. NAM v1.0.0 schema slot for the first lint wave (Resolution 1).
3. Fixed-cap DiagnosticBatch with `Internal`-status overflow (Resolution 2).
4. Reused viola host_ctx (Resolution 3).
5. Per-lint provider ids namespaced by pack origin (Resolution 4).
6. `mockspace.toml [plugins]` transitional, `viola.toml` post-integration (Resolution 5).
7. First-port lint (no-todo) end-to-end against the locked spec.

After DOC CL locks, the SRC CL ports no-todo against the resolutions. Subsequent rounds port the remaining built-ins and stack lints against the same pattern, with NAM schema enrichment when a lint genuinely needs it (lock criterion: the lint's port slice references the NAM minor revision it requires; v1.1 enrichment lands as its own design round).

## Open after these resolutions

These are deliberately not addressed in this memo, on grounds they belong to subsequent ports or to viola-side rounds:

- The `ViolaEngine` impl crate boundary (working name `viola-mockspace-engine`; lives in viola). Spec for that crate is a viola-side round under task #198, not mockspace #610. Mockspace's DOC CL names the consumer contract; viola's round implements it.
- Per-lint config-bytes wire format. The sketch passes `(*const u8, arvo::USize)` as opaque. Whether the bytes are TOML, bincode, or a purpose-built schema is a per-lint concern; the cdylib boundary stays format-agnostic. Each ported lint picks its own format in its port slice.
- Cdylib signing and integrity verification. Open beyond #610's scope.

## See also

- `mock/research/202605251000_lint-catalog-cdylib-boundary.md` (slice 1, Path A recommendation).
- `mock/research/sketches/202605251030_no-todo-cdylib-shape.md` (slice 2, feasibility sketch).
- `hilavitkutin/mock/research/202605232100_workunit-cdylib-boundary.md` (#609 boundary recommendation).
- `viola/mock/crates/viola-plugin-abi/src/nam.rs:34` (the opaque `NamPayload` carrier the schema resolution lives behind).
- `viola/mock/crates/viola-plugin-abi/src/vtable.rs:102` (the `LintEvaluateVtable` the cdylib targets).
- Task #198 (viola integration), Task #200 (viola.toml migration).
