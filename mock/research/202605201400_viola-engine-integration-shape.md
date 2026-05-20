# Viola engine integration shape

**Date:** 2026-05-20 (refreshed)
**Status:** Locked design; placeholder shipping
**Scope:** Mockspace v2 Phase 2, lint engine integration with viola
**Companion files:**
- `mockspace/mock/crates/mockspace-core/src/lint.rs` (the LintEngine surface)
- `mockspace/mock/research/202605201500_lint-catalog-migration-plan.md` (catalog migration; sibling note)
- `viola/crates/viola-core/src/pipeline.rs` (current viola pipeline)
- `viola/docs/HILAVITKUTIN-APP-SHAPE.md` (viola's structural target)
- `viola/docs/PLUGIN-ABI-V1-DESIGN.md` (the plugin ABI v1, frozen)

## Why this note exists

This note replaces an earlier proposal that recommended a dual `check` / `check_project` trait surface to bridge mockspace's per-file dispatch with viola's project-level dispatch. That proposal was applied in a sharper form: the substrate vocabulary was re-shaped directly around viola's NAM model so a single project-scoped `Lint::check` covers both engines without a dual-method bridge. The result landed at commit `6c21786` on `feat/ref-based-mockspace-redesign-proposal`.

This note documents what the integration actually looks like after that reshape, and serves as the durable design record future agents read when authoring viola-side or new lints.

## 1. The viola-native vocabulary, as it sits today

`mockspace-core/src/lint.rs` defines the trait surface that both engines bind to. The vocabulary mirrors viola's NAM schema (`viola/nam/v1`) one-to-one:

| Substrate type | Source / counterpart |
|---|---|
| `Document` | viola's NAM `Document` (path + language + source + content hash) |
| `Project` | viola's NAM project shape (root + surface + documents) |
| `Language` | viola's grammar plugin language tag |
| `ContentHash([u8; 32])` | viola's BLAKE3-style content digest |
| `RunSurface` (Local/Ci/Editor) | viola's `RunSurface` |
| `Finding` | strict superset of viola's `Diagnostic` |
| `Severity` (Off/Info/Warn/Error) | aligned with viola's `DiagnosticSeverity` |

The substrate is intentionally not a parallel model with a translation layer; it IS the model. When viola lands, `ViolaProject` impls `Project` directly from NAM, and `ViolaDocument` impls `Document` directly from a NAM document entry. No adapter crate, no facade.

## 2. The trait shape

Two authorship traits. One engine trait.

### `Lint`: project-scoped, the universal shape

```rust
pub trait Lint: Send {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn default_severity(&self) -> GateSeverity;
    fn check(
        &self,
        ctx: &LintContext<'_>,
        project: &dyn Project,
        sink: &mut dyn FindingSink,
    ) -> Result<(), LintError>;
}
```

Every lint sees the whole project. A per-document concern iterates `project.documents()` itself; a cross-document concern walks documents pairwise; a NAM-driven concern (when viola lands) reaches for the future typed accessors on `Project`. There is one method, not two; one decision per lint about what to look at, not two.

### `PerDocumentLint` + `PerDocumentLintBlanket`: a convenience marker

```rust
pub trait PerDocumentLint: Send {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn default_severity(&self) -> GateSeverity;
    fn check_document(
        &self,
        ctx: &LintContext<'_>,
        document: &dyn Document,
        sink: &mut dyn FindingSink,
    ) -> Result<(), LintError>;
}

pub trait PerDocumentLintBlanket: PerDocumentLint {}

impl<T: PerDocumentLintBlanket> Lint for T { /* walks project.documents() */ }
```

Lints that are genuinely per-file (forbids-tab, file-size, raw-source pattern detection) write `impl PerDocumentLint + PerDocumentLintBlanket`. The blanket impl turns them into `dyn Lint` automatically by iterating documents. Authorship cost stays "one method"; engine still sees a `dyn Lint` for dispatch.

The marker trait disambiguates the blanket from any direct `impl Lint for T`: if a type wants both per-document logic AND project-level state, it impls `Lint` directly and skips the marker. There is no implicit "every PerDocumentLint becomes a Lint"; the marker is the opt-in.

### `LintEngine`: the single swap point

```rust
pub trait LintEngine: 'static + Send + Sync {
    type Project: Project;
    type ParseError: std::error::Error + Send + Sync + 'static;
    type LoadError: std::error::Error + Send + Sync + 'static;
    type DispatchError: std::error::Error + Send + Sync + 'static;

    fn new() -> Result<Self, Self::LoadError> where Self: Sized;
    fn scope_project(&self, root: &Path, surface: RunSurface) -> Result<Self::Project, Self::ParseError>;
    fn run(&self, project: &Self::Project, lints: &[&dyn Lint], gate: Gate, cfg: &dyn LintCfgStore, sink: &mut dyn FindingSink) -> Result<RunReport, Self::DispatchError>;
    fn load_builtin_lints(&self) -> Result<Vec<Box<dyn Lint>>, Self::LoadError>;
}

pub type ActiveEngine = MockspaceEngine;
```

`ActiveEngine` is the single line that swaps engines. Today it points at `MockspaceEngine` (the std-backed placeholder); the day viola ships its engine crate, it becomes `viola_mockspace_engine::ViolaEngine`. Everything compiled against `Lint`, `Project`, `Document`, `Finding` keeps working.

## 3. What "viola plugs in natively" means concretely

When viola lands a `viola-mockspace-engine` crate, the integration is one trait impl per type:

```rust
pub struct ViolaEngine {
    host: ExtensionHost,
    runner: Option<Extension>,
    lint_plugins: Vec<LoadedLint>,
}

pub struct ViolaProject {
    workspace_root: PathBuf,
    surface: RunSurface,
    documents: Vec<Box<dyn Document>>, // ViolaDocument values from NAM
    nam_payload: NamPayloadOwned,      // raw NAM bytes; future Project accessors read from here
}

pub struct ViolaDocument {
    path: PathBuf,
    language: Language,
    source: String,                    // deep-copied from NAM document.source (or "" if NAM omits)
    content_hash: ContentHash,         // copied from NAM document.content_hash
}

impl Document for ViolaDocument {
    fn path(&self) -> &Path { &self.path }
    fn language(&self) -> Language { self.language }
    fn source(&self) -> &str { &self.source }
    fn content_hash(&self) -> &ContentHash { &self.content_hash }
}

impl Project for ViolaProject {
    fn root(&self) -> &Path { &self.workspace_root }
    fn surface(&self) -> RunSurface { self.surface }
    fn documents(&self) -> &[Box<dyn Document>] { &self.documents }
    // Future NAM-aware accessors land here additively when the NAM schema concretises.
}

impl LintEngine for ViolaEngine {
    type Project = ViolaProject;
    type ParseError = ViolaParseError;
    type LoadError = ViolaLoadError;
    type DispatchError = ViolaDispatchError;

    fn new() -> Result<Self, Self::LoadError> { /* wire ExtensionHost; runner / lints registered via builder */ }

    fn scope_project(&self, root: &Path, surface: RunSurface) -> Result<ViolaProject, Self::ParseError> {
        // 1. Build RunScope from root + a host-side file walker.
        // 2. Resolve runner vtable, call execute_scope(host_ctx, scope, out_nam) once.
        // 3. Deserialise NamPayload; deep-copy each NAM document into a ViolaDocument.
        // 4. Return ViolaProject { workspace_root, surface, documents, nam_payload }.
    }

    fn run(&self, project: &ViolaProject, lints: &[&dyn Lint], gate: Gate, cfg: &dyn LintCfgStore, sink: &mut dyn FindingSink) -> Result<RunReport, Self::DispatchError> {
        // For each lint: resolve severity vs cfg; skip if Off at gate; call lint.check(ctx, project, sink).
        // Lints from FFI cdylibs go through ExtensionToLintAdapter (below).
    }

    fn load_builtin_lints(&self) -> Result<Vec<Box<dyn Lint>>, Self::LoadError> {
        // Wrap each loaded viola lint extension as a Box<dyn Lint>.
        Ok(self.lint_plugins.iter().map(|loaded| {
            Box::new(ExtensionToLintAdapter { extension: loaded.extension.clone(), name: loaded.name, host_ctx: self.host_ctx }) as Box<dyn Lint>
        }).collect())
    }
}
```

The `ExtensionToLintAdapter` is the load-bearing piece. Viola lint plugins ship as cdylibs exporting `PROVIDER_LINT_EVALUATE`. The adapter wraps an `Extension` into a `dyn Lint`:

```rust
struct ExtensionToLintAdapter {
    extension: Extension,
    name: &'static str,
    host_ctx: *mut c_void,
}

impl Lint for ExtensionToLintAdapter {
    fn name(&self) -> &'static str { self.name }
    fn description(&self) -> &'static str { /* from descriptor metadata */ }
    fn default_severity(&self) -> GateSeverity { /* from descriptor metadata */ }
    fn check(&self, ctx: &LintContext, project: &dyn Project, sink: &mut dyn FindingSink) -> Result<(), LintError> {
        // 1. Downcast project to ViolaProject (engine guarantees this at registration).
        // 2. Get this lint's config from ctx.config via name lookup; serialise to BytesRef.
        // 3. Call LintEvaluateVtable::evaluate(host_ctx, nam_ptr, cfg.data, cfg.len, &mut batch).
        // 4. Translate each viola Diagnostic in `batch` to a mockspace Finding (deep-copy BytesRef into Cow::Owned strings, map severity, preserve plugin_id/rule_id/metadata).
        // 5. sink.emit(finding) for each.
        // 6. Map errors to LintError.
    }
}
```

The diagnostic → finding translation is total. `Severity` ordinals align (`Off=0, Info=1, Warn=2, Error=3`); `Span` carries the same `(file, start_line, start_col, end_line, end_col)` shape viola's `SourceRange` carries; `plugin_id` and `rule_id` pass through unchanged; multi-line spans, related spans, and metadata blobs round-trip without loss. The only heap allocation in the path is the `BytesRef → Cow::Owned<str>` copy needed because `Finding`'s `Cow<'static, str>` fields cannot borrow from a plugin-owned arena that outlives `evaluate`. Documented constraint; only allocation in an otherwise allocation-clean dispatch loop.

## 4. The downcast question, honestly

A `dyn Lint` does not know its engine. The adapter above needs `project: &ViolaProject`, not `project: &dyn Project`. Two paths:

**Path 1: engine-typed registration.** The engine's `load_builtin_lints` returns adapters that capture the engine's concrete `Project` type by carrying a function pointer with the right signature. The adapter holds `fn(&ViolaProject, ...) -> ...` and the runtime dispatch goes via this pointer. The `dyn Lint::check` body downcasts via the function pointer's existence.

**Path 2: NAM passed through the substrate.** `Project` grows an optional `fn nam_payload(&self) -> Option<&[u8]>` accessor. `ViolaProject` returns `Some`; `MockspaceProject` returns `None`. Adapters check it; if absent, return `LintError::AnalysisFailure`.

Path 2 reintroduces the `MetadataBlob` escape hatch the reshape deliberately removed. Path 1 keeps the substrate clean at the cost of plugin-side complexity (each lint registration knows its engine).

Decision deferred to when `viola-mockspace-engine` is authored. The substrate compiles fine without resolving it (both paths are additive). The right move is probably Path 1: keep the substrate honest, push the engine-aware adapter complexity into the engine crate where it belongs.

## 5. Engine construction is engine-specific

The "swap via one alias line" claim has one honest qualifier: `LintEngine::new()` takes no parameters, so engine-specific configuration (which runner plugin to load, which lint cdylibs to register) lives on engine-specific builder methods that are not part of the trait.

```rust
// In viola-mockspace-engine, not in mockspace-core:
impl ViolaEngine {
    pub fn with_runner(self, path: &Path) -> Result<Self, ViolaLoadError> { /* ... */ }
    pub fn with_lint(self, name: &str, path: &Path) -> Result<Self, ViolaLoadError> { /* ... */ }
}
```

This is unavoidable. The trait abstracts the run, not the construction. Consumers swap `ActiveEngine`, but if they also swap engines they may need to swap their builder calls. Documented behaviour; not a bug.

`MockspaceEngine::new()` constructs from nothing because its inputs (the file walker, the parser tier) are not consumer-supplied; the engine is a self-contained std-backed runner. Honesty in the README + lint module docs covers the user-visible delta.

## 6. The lint catalog: what runs through this engine

The substrate-shape question is settled. The catalog-shape question lives in a sibling note: `mock/research/202605201500_lint-catalog-migration-plan.md`. Key facts the integration story depends on:

- 63 lints exist today across three pools: mockspace built-ins (33), the shared stack-lints pack (18), and per-repo custom lints (12, across arvo and hilavitkutin).
- All currently target `LINT_CONTRACT_VERSION = 1` (the pre-reshape trait shape).
- The v2 reshape ships the trait surface above. Lints will migrate; the migration is mechanical for per-document shapes and additive for project-scoped concerns.
- The migration plan categorises which lints stay built-in to mockspace, which move to the shared stack-lints pack, and which dissolve into deduplication or drop entirely.

The substrate's job is to give that catalog migration a stable target. It does. The reshape will not move again before lints migrate.

## 7. End-to-end flow, captured for reference

```
                          ┌──────────────────────────┐
                          │ mockspace.toml           │
                          │ + lints/*.rs (custom)    │
                          │ + lint-crates (external) │
                          └─────────────┬────────────┘
                                        │
                                        v
                          ┌──────────────────────────┐
                          │ ActiveEngine::new()      │
                          │  (= MockspaceEngine or   │
                          │     ViolaEngine)         │
                          └─────────────┬────────────┘
                                        │
                                        v
                          ┌──────────────────────────┐
                          │ scope_project(root, ↓)   │
                          │  Mockspace: disk walk    │
                          │  Viola: runner.execute_  │
                          │         scope -> NAM     │
                          └─────────────┬────────────┘
                                        │ Project
                                        v
                          ┌──────────────────────────┐
                          │ run(project, lints,      │
                          │     gate, cfg, sink)     │
                          │  per lint:               │
                          │    resolve_severity      │
                          │    lint.check(...)       │
                          │      |                   │
                          │      v                   │
                          │   sink.emit(Finding)*    │
                          └──────────────────────────┘
```

The arrow from "mockspace.toml" through `ActiveEngine` to dispatch is identical for both engines. Lint authors target the trait surface; consumers configure via `mockspace.toml`; engines do their thing under the hood.

## 8. What the integration costs

**On the mockspace side, going forward:** zero further substrate surface changes are anticipated before viola lands. Phase 2B (tree-sitter backend for MockspaceEngine), Phase 2D (built-in lint catalog), and the per-repo migration are all additive; they consume the trait surface but do not modify it.

**On the viola side, when it lands:** one new crate (`viola-mockspace-engine`), four trait impls (`Document`, `Project`, `LintEngine`, plus the FFI `ExtensionToLintAdapter`), and one builder API (`with_runner` / `with_lint`). The viola pipeline (`pipeline.rs:64-155`), the plugin ABI (`viola-plugin-abi/*`), the extension host (`hilavitkutin-extensions`), and the FFI vtables stay untouched.

**On the consumer-of-mockspace side:** the `ActiveEngine` alias change is one line. Engine-specific construction is the visible delta if the consumer uses `with_runner` / `with_lint`; the workspace's `mock/agent/` scaffolding hides this when the runner choice is part of project config rather than code.

## 9. Honest engineering assessment

The earlier proposal's framing, "the per-file structuring is lossy when forced over viola; add a second method to bridge", was correct about the problem and one step short on the solution. The right solution was not "add a second method to bridge two shapes" but "pick the shape that doesn't need bridging". Project-scoped IS the shape that doesn't need bridging; per-document is a convenience that synthesises on top via the blanket impl.

The result is structurally smaller (one method on `Lint`, not two) and the engine-integration story is structurally cleaner (the substrate vocabulary is viola's vocabulary; the placeholder produces it from disk; the eventual viola engine produces it from NAM). The "swap is one type alias line" claim holds at the substrate level; engine-specific construction is the honest qualifier that lives in the engine docs, not the substrate.

Nothing about this design forecloses on future evolution. NAM-aware accessors on `Project` land additively. Severity/gate/surface vocabulary is wire-compatible across engines. The lint catalog migrates once and then runs through whichever engine `ActiveEngine` names.

## 10. References

- `lint.rs:9-83`: module doc; the design intent stated in code.
- `lint.rs:99`: `LINT_CONTRACT_VERSION = 2`.
- `lint.rs:393-432`: `Document` and `Project` traits.
- `lint.rs:528-538`: `Lint` trait.
- `lint.rs:546-586`: `PerDocumentLint` + `PerDocumentLintBlanket` + blanket impl.
- `lint.rs:619-663`: `LintEngine` trait.
- `viola/crates/viola-plugin-abi/src/vtable.rs:103-110`: `LintEvaluateVtable::evaluate` signature.
- `viola/crates/viola-plugin-abi/src/nam.rs:32-38`: `NamPayload` wire shape.
- `viola/crates/viola-core/src/pipeline.rs:64-155`: the pipeline function the eventual `ViolaEngine::run` replicates the dispatch of.
- `viola/docs/PLUGIN-ABI-V1-DESIGN.md:264-320`: NAM v1 schema document.
- Sibling note: `mock/research/202605201500_lint-catalog-migration-plan.md`.

## Recorded

2026-05-20 refresh after the viola-native reshape landed at `6c21786`. The original 2026-05-20 note proposed a dual-method bridge; this refresh records what shipped instead.
