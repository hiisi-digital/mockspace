# Engine architecture, suppression model, and the minimal substrate

**Date:** 2026-05-20 (revised post senior review)
**Status:** Proposal, pre-implementation
**Scope:** Mockspace v2 Phase 2 architectural refinement; supersedes the substrate-as-descriptors framing from the original revision of this note
**Sibling notes:**
- `mock/research/202605201400_viola-engine-integration-shape.md` (viola integration shape)
- `mock/research/202605201500_lint-catalog-migration-plan.md` (catalog migration; gets a lints.toml addendum below)

## Why this revision exists

The first revision of this note proposed a third trait-surface reshape: substrate as `#[repr(C)] LintDescriptor` + C ABI `LintEvaluateFn` claimed to mirror viola's ABI v1. A senior review found the claim was inaccurate (viola's actual `LintEvaluateVtable::evaluate` takes `*const NamPayload`, not engine-passed per-node handles) and identified ten settle-now items plus four reshape-fragility hot spots. The user's response went further: stop trying to make the substrate be the wire format AND the dispatch protocol AND the authoring interface. The substrate is none of those. It is the input/output vocabulary plus a one-method engine trait that takes a project and returns findings. Everything else is engine-internal.

This revision captures the simplified substrate. The shape is structurally smaller than every prior reshape this session has shipped, and it forecloses the failure mode the session has cycled through: every prior reshape was driven by the substrate trying to model something engine-shaped that turned out not to match what viola actually produces. With the substrate not modelling dispatch at all, that failure mode goes away.

## 1. The minimal substrate

```rust
// In mockspace-core. The complete substrate trait surface.

pub trait LintEngine {
    /// The project this engine produces from a workspace root. Concrete
    /// projects implement Project so consumers see a uniform shape;
    /// the engine itself owns the concrete type.
    type Project: Project;

    /// Errors returned by the engine's three operations. Engine-specific
    /// so the engine can name its own failure modes precisely; consumers
    /// see them through Box<dyn std::error::Error> at the entrypoint.
    type ParseError: std::error::Error + Send + Sync + 'static;
    type LoadError: std::error::Error + Send + Sync + 'static;
    type DispatchError: std::error::Error + Send + Sync + 'static;

    /// Construct a fresh engine. No parameters; engine-specific
    /// configuration is encoded in the engine's typestate parameters
    /// (i.e. the type you wrote `type LintEngine = ...;` to choose).
    fn new() -> Result<Self, Self::LoadError> where Self: Sized;

    /// Walk the project root, do whatever preparation the engine does
    /// (disk walk, runner invocation, NAM deserialisation, macro
    /// expansion, etc.), produce the engine's Project. The surface
    /// argument is recorded so lints can branch on it.
    fn scope_project(
        &self,
        root: &Path,
        surface: RunSurface,
    ) -> Result<Self::Project, Self::ParseError>;

    /// Run every lint the engine is configured to run. Returns the
    /// findings (with suppressions already applied). Whether lints
    /// execute in parallel, share a single tree walk, dispatch via
    /// node-interest tables, run as cdylibs, or anything else is the
    /// engine's choice. The substrate doesn't know.
    fn run(
        &self,
        project: &Self::Project,
        gate: Gate,
        cfg: &dyn LintCfgStore,
    ) -> Result<Vec<Finding>, Self::DispatchError>;
}
```

That is the whole engine trait. Five methods, four associated types, zero opinions about how lints work internally.

Consumer code is:

```rust
let engine = <ActiveEngine as LintEngine>::new()?;
let project = engine.scope_project(root, RunSurface::Local)?;
let findings = engine.run(&project, Gate::Push, &cfg)?;
```

The `ActiveEngine` alias is set at the top of the launch chain to whichever engine is active (mockspace's std-backed engine today; viola's engine once viola integrates). Engine swap is one line. No descriptor format. No dispatch protocol. No node-visitor callback shape. No FFI vtable in the substrate.

Inside the engine, anything is fair game: per-document tree walks, NAM-payload-once dispatch, parallel rayon-driven evaluation, cdylib plugin loading, async dispatch, single-threaded sequential walks. None of it is observable through the trait. The engine's internals can change without the substrate moving.

## 2. What stays in the substrate

The substrate ships these types (all in `mockspace-core/src/lint.rs`):

**Input vocabulary** (what engines consume):

- `Document`: trait. Path, language, source bytes, content hash. Engines impl this on whatever concrete document they produce.
- `Project`: trait. Root, surface, documents slice. Engines impl this on whatever concrete project they produce.
- `Language`: enum matching viola's grammar plugins (Rust, TypeScript, etc.).
- `RunSurface`: enum (Local | Ci | Editor).
- `ContentHash([u8; 32])`: uniform 32-byte hash carrier. Each engine documents which algorithm fills it via a `const HASH_ALGORITHM: HashAlgorithm` associated constant (see §5).

**Output vocabulary** (what engines return):

- `Finding`: the result type. Lint name, plugin id (optional for non-FFI lints), rule id, severity, message, span, optional fix suggestion, related spans, optional metadata blob.
- `Span`: file path + start line/col + end line/col. Identical to viola's `SourceRange` shape.
- `Severity`: enum matching viola's TOML v2 vocabulary (Error | Warn | Info | Hint | Off | Skip). Six variants.
- `Impact`: enum matching viola's TOML v2 impact axis (Critical | Major | Minor | Trivial). Optional on findings.
- `Category`: enum matching viola's TOML v2 category axis (Correctness | Maintainability | Consistency | Performance | Style). Optional on findings.
- `Gate`: enum (Commit | Build | Push).
- `GateSeverity`: per-gate severity triple.

**Config carrier**:

- `LintCfgStore`: trait. Engines and consumers pass concrete impls; the trait is just the getter shape. Implementations live in mockspace-core (for TOML-backed config) and per-engine if needed.

**Suppression model** (§4 details):

- `SuppressionMap`: substrate-level type. Scope-set with innermost-enclosing resolution. Engines populate; consumers consult; engine consults before emitting to the returned `Vec<Finding>`.
- `SuppressionScope`: span + lint-name set.

**Errors**:

- `LintError`: universal error type findings can carry. Variants: AnalysisFailure | BadConfig | Io | Internal.

That is the complete substrate. Roughly 300 lines once shipped, mostly enum variants and small struct fields. No trait machinery for lint authorship. No dispatch protocol. No descriptor format.

## 3. What leaves the substrate

Everything currently in `lint.rs` that describes HOW lints execute moves into engine-internal code. Specifically:

- `Lint` trait, `PerDocumentLint` trait, `PerDocumentLintBlanket` marker: these become **authoring conveniences inside the Rust engine** (mockspace-rs). They are not substrate types. Lints written for `MockspaceEngine` use these; lints written for `ViolaEngine` use whatever shape viola's plugin SDK provides.
- `LintDescriptor`, `LintEvaluateFn`, `NodeInterest`: engine-internal dispatch concerns, only relevant when an engine implements FFI lint loading. Each engine ships its own descriptor format if it needs one.
- `LanguagePreprocessor` trait, `PreprocessorRegistry`: mockspace-rs's internal organisation. `MockspaceEngine` walks Rust documents via mockspace-rs's preprocessor; the substrate doesn't know how, doesn't care.
- Single-walk dispatch, node-interest tables, multi-view tree walks (TokenStream view + syn::File view): engine-internal optimisations. mockspace-rs chooses whatever it wants; substrate sees `Vec<Finding>` at the output.
- `load_builtin_lints` method on `LintEngine`: gone. Built-in lints are an engine implementation detail. Consumers don't ask the engine "what lints are you running"; they configure the engine via `cfg`, and the engine decides what to run.

The mockspace-rs crate becomes the home of all this. mockspace-rs ships `MockspaceEngine` (the std-backed implementation), the `LanguagePreprocessor` trait it uses internally, the Rust-side `Lint`/`PerDocumentLint` authoring traits, the proc-macro for `#[mock::lints::allow(...)]` recognition, and the no-suffix/no-prefix configurable lint family's matcher primitive. It is the Rust-side engine implementation; the substrate is unaware of any of it.

When viola integrates, `viola-mockspace-engine` ships `ViolaEngine` with whatever internal shape it needs (cdylib plugin loading, NAM-payload dispatch, ABI v1 wire format). It implements the same `LintEngine` trait the substrate ships. The substrate sees `Vec<Finding>` at the output, same as it does from `MockspaceEngine`.

## 4. Suppression model

Suppression IS substrate-level. The `#[mock::lints::allow(...)]` attribute (Rust) and `// lint:allow(...)` comment (any language) must work identically regardless of which engine ran the lint. Cross-engine portability requires the substrate to own the model.

The substrate ships:

```rust
pub struct SuppressionMap {
    scopes: Vec<SuppressionScope>,
}

pub struct SuppressionScope {
    /// The span this suppression covers. For an attribute on a fn,
    /// the fn's span. For a module-level allow, the module's span.
    /// For a crate-level allow, the crate root's span.
    pub scope: Span,
    /// Lint names suppressed within this scope.
    pub lints: BTreeSet<String>,
    /// Optional tracking task ID (the `tracked = "#N"` parameter).
    pub tracked: Option<String>,
    /// Optional human-readable reason.
    pub reason: Option<String>,
}

impl SuppressionMap {
    /// Resolve whether a (lint_name, finding_span) pair is suppressed.
    /// Returns the innermost enclosing scope that covers the finding;
    /// None if no scope suppresses.
    pub fn resolves(&self, lint_name: &str, finding: &Span) -> Option<&SuppressionScope>;
}
```

Resolution semantics match Rust's `#[allow(...)]`:

- Suppression scopes nest. A finding at line 42 inside a function inside a module inside a crate root may be covered by any combination of allows at any of those levels.
- The innermost enclosing scope that suppresses the finding's lint name wins. (Rust's `#[allow]` is additive: inner adds to outer's suppressions. The same applies here.)
- A scope suppresses a finding if the finding's span is fully contained in the scope's span AND the finding's lint name is in the scope's `lints` set.
- Findings emitted with no enclosing suppression scope reach the returned `Vec<Finding>` unfiltered.

Meta-lint support: `LintEngine::run` exposes the `SuppressionMap` as part of the result (return a pair: `(Vec<Finding>, SuppressionMap)`) or via an accessor method on the engine. Either way, meta-lints (`overuse-of-allow`, `untracked-allow`, `expired-tracked-allow`) read the map as their input. They run as ordinary lints inside the engine's normal dispatch.

Population: language-specific. Rust source has `#[mock::lints::allow(...)]` attributes; mockspace-rs's preprocessor reads them. Other languages have `// lint:allow(...)` comments; per-language preprocessors parse them. Both populate the same `SuppressionMap` type. Engine merges per-document maps into one project-level map before resolving findings.

The attribute syntax (Rust side):

```rust
#[mock::lints::allow(no_bare_string, tracked = "#509", reason = "FFI boundary")]
pub fn returns_legacy_str() -> String { /* ... */ }

#[mock::lints::allow(no_bare_string, no_alloc, tracked = "#477")]
pub fn alloc_bridge() -> Box<[u8]> { /* ... */ }

// Crate-level (suppresses everywhere in the crate):
#![mock::lints::allow(no_todo, tracked = "#999")]
```

The attribute is an empty marker. mockspace-rs ships a no-op proc-macro that exists solely to make rustc accept the attribute; the preprocessor reads the attribute's tokens directly during walk. Consumers add `mockspace-rs-attrs = { ... }` as a dev-dep (or behind a `mockspace` cfg gate). No compile-time codegen; no runtime cost; one tiny dep.

The `tracked` parameter is mandatory per the `lint-allow-requires-task-id` workspace rule. mockspace-rs's preprocessor rejects allow attributes without `tracked = ...` at scope-population time, producing a meta-finding the consumer sees in the usual `Vec<Finding>`.

## 5. Severity vocabulary (adopting viola TOML v2)

Mockspace adopts viola's TOML v2 severity schema directly. From `viola/docs/VIOLA-TOML-V2-SCHEMA.md:226-258`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Block at gate threshold.
    Error,
    /// Report; do not block.
    Warn,
    /// Informational.
    Info,
    /// Dim suggestion.
    Hint,
    /// Suppress entirely.
    Off,
    /// Suppress and short-circuit the linter run on this file.
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Impact {
    Critical,  // index 0 (highest)
    Major,     // index 1
    Minor,     // index 2
    Trivial,   // index 3 (lowest)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Correctness,
    Maintainability,
    Consistency,
    Performance,
    Style,
}

#[derive(Debug, Clone, Copy)]
pub enum HashAlgorithm {
    Blake3,
    Fnv1a,
    /// Future engines may add variants. `#[non_exhaustive]` ensures
    /// extending the enum doesn't break pattern matches in consumer
    /// code.
}
```

Each engine declares its hash algorithm:

```rust
impl LintEngine for MockspaceEngine {
    const HASH_ALGORITHM: HashAlgorithm = HashAlgorithm::Blake3;
    // ...
}
```

`HASH_ALGORITHM` is an enum (per the senior review), not a string constant. `#[non_exhaustive]` so future engines extend without breaking pattern matches.

Wire-level alignment with viola's current Rust ABI: viola's `DiagnosticSeverity` ships with three variants (Info | Warn | Error) at ABI v1. The substrate's `Severity` has six. Off / Skip / Hint are host-side: they never cross the FFI boundary; they exist either before evaluate-call (Off/Skip resolve via cfg → engine never invokes the suppressed lint) or after-evaluate at finding-emission time (Hint is a host-side display concession). When viola grows its ABI to v1.1 to add the missing variants, mockspace's translation table becomes 1:1. Until then, the three shared variants map directly and the three host-only variants stay host-side.

## 6. Lints config file: `lints.toml`

Lint configuration moves out of `mockspace.toml` into a separate `lints.toml` at the project root. The schema matches viola's TOML v2 verbatim. When viola integrates, the file renames to `viola.toml`; the schema does not change.

```toml
# lints.toml at the project root (placeholder name; renames to viola.toml).

[lints]
default_severity = "warn"

[lints."no-bare-string"]
severity = "error"
impact = "major"
category = "correctness"
exempt_paths = ["src/ffi/**"]

[lints."no-suffix"]
severity = "warn"
config = { forbidden = ["Entry", "Manager"], scope = "public" }

# Pattern-shaped overrides per the TOML v2 grammar:
[overrides]
"duplicate-logic/*" = "off"
"*::correctness" = "error"
"*>=major" = "warn"
```

`mockspace.toml` retains workflow concerns only (lint pack pinning, mockspace's own scaffolding, etc.). The split:

- `mockspace.toml`: mockspace workflow + tooling config. Stays.
- `lints.toml`: lint engine config matching viola TOML v2 schema. New.
- `viola.toml`: identical to `lints.toml`; the rename happens when viola integrates as the active engine. The schema is the same; consumers just rename the file.

For consumer repos: the catalog migration plan's per-repo update step (sibling note §"Order of operations" step 4) absorbs the `lints.toml` extraction. One `mv mockspace.toml.lints lints.toml` step per repo; the file's contents move verbatim except the section path changes from `[lint.<name>]` to `[lints.<name>]` (plural, viola convention).

## 7. Engine-internal concerns (explicitly not substrate)

The senior review's reshape-fragility items #7 (multi-view walks per language) and #8 (authoring ergonomics regression) and the implicit "how does cdylib lint loading work" question all collapse to this: every concern about HOW lints execute is engine-internal.

Concretely:

- **Tree walks** (single, multi-view, parallel, etc.): mockspace-rs (the Rust engine) decides. mockspace-ts (future TS engine) decides. Substrate sees `Vec<Finding>`.
- **Node-interest tables**: mockspace-rs's internal optimisation. Other engines may or may not need them. Substrate doesn't know.
- **Lint registration**: each engine ships its own catalog mechanism. `MockspaceEngine` collects `impl Lint` blocks from its `mod builtins`; `ViolaEngine` loads cdylibs at startup. The substrate sees neither.
- **FFI ABI shapes**: `ViolaEngine` consumes viola's ABI v1 shape internally; that's a viola-mockspace-engine concern. mockspace's `LintEngine` trait doesn't mention it.
- **Lint authorship traits**: mockspace-rs's `Lint`, `PerDocumentLint`, `PerDocumentLintBlanket` traits are mockspace-rs-internal. Lints written against them only run on `MockspaceEngine`. When viola ships its plugin SDK, lints written against THAT SDK run on `ViolaEngine`. Cross-engine lint portability is a separate concern (it requires lints to be re-authored against each engine's SDK, OR cdylib-shipped lints that both engines can load).
- **Multi-view walks** (TokenStream + syn::File): mockspace-rs decides how it walks Rust documents. If it needs two views, it does two walks internally. The substrate sees one `run` call and `Vec<Finding>` out.

This is the structural principle: **anything that could vary across engines is engine-internal**. The substrate ships only what MUST be uniform across engines (vocabulary, suppression model, config format).

## 8. The no-suffix / no-prefix configurable family

Lives in stack-lints (since it enforces workspace-discipline). The substrate ships only the matcher primitive:

```rust
// In mockspace-core.

pub fn matches_pattern(name: &str, forbidden: &[String], exempt: &[String]) -> bool {
    if exempt.iter().any(|e| e == name) {
        return false;
    }
    forbidden.iter().any(|f| name == f || name.ends_with(f) || name.starts_with(f))
}
```

Stack-lints uses this for `no-suffix` and `no-prefix`. The lint impls are mockspace-rs-shaped (authored against the Rust engine's `Lint` trait). Configurable forbidden/exempt lists + visibility scoping live in the lint impls, not in the substrate.

For glob/regex matching (the senior review's reshape-fragility item): defer. The matcher primitive ships exact-string only at v1. If a consumer needs globs, the matcher grows to support them additively (substrate-level decision when the first glob need lands; not now).

## 9. What this revision settles vs defers

**Settles now** (vs the senior review's 10 settle-now items):

| Review item | How settled |
|---|---|
| C-ABI lie (LintEvaluateFn ≠ viola's evaluate) | Substrate ships no FFI shape. Engine-internal. |
| Severity mapping (4 vs 3 variants) | Substrate adopts viola TOML v2 (6 variants). Three are host-only. |
| String ownership at descriptor | No descriptor in substrate. Engine-internal. |
| HASH_ALGO_NAME as string | Enum with `#[non_exhaustive]`. |
| NodeInterest u32 namespace collision | No NodeInterest in substrate. Engine-internal. |
| SuppressionMap shape | Scope-set with innermost-enclosing resolution. |
| Meta-lint exposure | `LintEngine::run` returns `(Vec<Finding>, SuppressionMap)` or exposes via accessor; meta-lints read normally. |
| One walk per language | Engine-internal; substrate doesn't model walks. |
| Project-scoped null pointer | No fn-pointer in substrate. Engine-internal. |
| extern "C" fn return type | No FFI in substrate. Engine-internal. |

**Reshape-fragility hot spots from the senior review**:

| Item | Status |
|---|---|
| Configurable forbidden lists (glob/regex pressure) | Defer (matcher ships exact-only; grows additively). |
| Attribute strategy C silencing dep | Accept; one tiny dev-dep per consumer; document. |
| `document.source()` post-preprocessing vs raw | Engine-internal. mockspace-rs documents its conventions. Substrate just says "document.source() is what the engine produces". |
| DiagnosticBatch streaming | Engine-internal (when viola integrates). Substrate sees `Vec<Finding>` only. |

**Defers** (genuinely future-round):

- Cross-engine lint portability mechanism (cdylib lints loadable by both engines, vs SDK-per-engine, vs ...).
- Per-finding severity vs per-lint severity (`Finding` already carries severity; pattern usage is informal).
- Workspace-aware multi-config aggregation (viola defers this too; mockspace #222 will revisit).

## 10. Honest engineering assessment

The simplification removes more than it adds. The substrate goes from ~1600 lines of trait machinery at HEAD to roughly 300 lines of types under this proposal. The `Lint` and `PerDocumentLint` traits + `PerDocumentLintBlanket` marker + `LintDescriptor` + `LintEvaluateFn` + `NodeInterest` + `LanguagePreprocessor` + `PreprocessorRegistry` all move to mockspace-rs as engine-internal implementation. The substrate stays small and stable.

This is not a third reshape on top of two. It is a recognition that the prior reshapes were modelling things they shouldn't have been. Each prior reshape was driven by realising the substrate didn't match viola's shape. With the substrate not modelling viola's shape at all (only the input/output vocabulary plus engine-trait shape), that pressure goes away. Future viola ABI changes (v1.1 adding Off/Skip/Hint at the wire; eventual v2) do not force substrate reshapes; they force engine-internal updates in `viola-mockspace-engine`.

The cost: lint catalogue migration is now engine-shaped, not substrate-shaped. The catalog migration plan (sibling note) needs a small revision: "port to v2 trait surface" becomes "port to mockspace-rs's Rust authoring conventions". Lints written for MockspaceEngine can't directly run on ViolaEngine until the cross-engine portability mechanism lands (probably cdylib lints loadable by both engines, eventually). For the foreseeable future, mockspace's built-in lint catalog runs on MockspaceEngine; viola's plugin ecosystem runs on ViolaEngine; they don't share code but they share output shape (`Vec<Finding>`).

The reshape ships in Phase 2A6 as a substrate simplification, NOT a new descriptor format. Concrete diff to current `lint.rs`:

- DELETE: `Lint` trait, `PerDocumentLint` trait, `PerDocumentLintBlanket` marker, blanket impl, `MockspaceProject`, `MockspaceDocument`, `MockspaceEngine` (these move to mockspace-rs).
- DELETE: `load_builtin_lints` method on `LintEngine`.
- ADD: `SuppressionMap`, `SuppressionScope`, `Impact`, `Category`, `HashAlgorithm`.
- CHANGE: `Severity` enum to 6 variants matching viola TOML v2.
- CHANGE: `LintEngine::run` to return `Vec<Finding>` directly (with suppressions applied).
- CHANGE: `LintEngine::HASH_ALGORITHM` associated constant (enum, not string).
- KEEP: `Document`, `Project`, `Language`, `RunSurface`, `ContentHash`, `Span`, `Finding`, `Gate`, `GateSeverity`, `LintCfgStore`, `LintError`, `LintContext`.

Roughly 1200 lines deleted, 200 added. Substrate becomes ~300 lines total. Tests adjust to the smaller surface; most existing test fixtures (the placeholder MockspaceEngine ones) move to mockspace-rs alongside the engine.

## 11. Concrete next steps

In order:

1. **Phase 2A6**: substrate simplification per §1 + §10. New `lint.rs` ships the minimal trait surface. mockspace-core's `Cargo.toml` loses the `serde`/`toml` features (unless needed for `LintCfgStore`'s default impl). Tests adjust.

2. **mockspace-rs crate creation** (`mockspace/mock/crates/mockspace-rs/`): hosts `MockspaceEngine`, the `Lint`/`PerDocumentLint` authoring traits, the `LanguagePreprocessor` trait (engine-internal), the Rust preprocessor impl, the matcher primitive used by stack-lints. Lifts the deleted-from-substrate items into this crate.

3. **mockspace-rs-attrs crate creation** (`mockspace/mock/crates/mockspace-rs-attrs/`): empty marker proc-macro for `#[mock::lints::allow(...)]`. Consumer crates depend on it (probably as dev-dep or `mockspace` cfg gate).

4. **Phase 2B**: `MockspaceEngine` ships real disk-walking + BLAKE3 hashing + Rust preprocessor + attribute parsing into `SuppressionMap`.

5. **Phase 2D**: port the 16 mockspace built-in lints to mockspace-rs's `Lint`/`PerDocumentLint` shape. The catalog migration plan's mechanical mapping (Pattern A/B/C/D) works unchanged at the lint-body level; the registration point moves to mockspace-rs's catalog.

6. **Stack-lints v2 migration**: port 17 stack-lints + 17 migrating-from-mockspace + new no-suffix/no-prefix family to mockspace-rs's authoring shape.

7. **Per-repo updates**: extract `lints.toml` from `mockspace.toml`. Update lint pack pin. Delete duplicate per-repo lints. Port repo-local lints.

8. **Viola integration** (when viola ships): `viola-mockspace-engine` crate implements `LintEngine`. Internal design entirely viola-shaped; substrate doesn't constrain it.

## 12. Addendum to the catalog migration plan

`mock/research/202605201500_lint-catalog-migration-plan.md` needs a small revision to reflect:

- The `lints.toml` extraction (consumer migration step 4).
- The descriptor format that section's "v1 → v2 cookbook" referenced is no longer substrate-level; the cookbook's mechanical mappings (Pattern A/B/C/D) target mockspace-rs's authoring traits, not substrate descriptors.
- The "13 mockspace built-ins + 17 stack-lints + per-repo" categorisation is unchanged; the destination crates and config files shift slightly (the workspace-discipline lints move into stack-lints as before, but they target mockspace-rs's Lint trait, not a substrate-level Lint trait).

The catalog migration plan can absorb these revisions in a small edit; the categorisation work it did is not affected.

## References

- `viola/docs/VIOLA-TOML-V2-SCHEMA.md:226-258`: severity / impact / category vocabulary mockspace adopts.
- `viola/docs/VIOLA-TOML-V2-SCHEMA.md:401-405`: viola explicitly defers per-issue suppress comments to source conventions, leaving the suppression model to the consumer ecosystem.
- `viola/crates/viola-plugin-abi/src/vtable.rs:64-114`: viola's actual ABI v1 vtable shape (cited only because the first revision of this note mis-claimed alignment; the simplified substrate does not depend on this shape).
- Sibling note: `mock/research/202605201400_viola-engine-integration-shape.md`.
- Sibling note: `mock/research/202605201500_lint-catalog-migration-plan.md`.
- Senior review findings (this session's chat transcript; the 10 settle-now items + 4 reshape-fragility hot spots inform §9).
- Workspace rule `hilavitkutin-workunit-mental-model.md`: the "engine is the runtime, substrate is the vocabulary" principle this simplification follows.

## Recorded

2026-05-20 first revision proposed substrate-as-descriptors. Senior review flagged the descriptor format diverged from viola's ABI v1 despite claiming to mirror it, plus 10 settle-now items. User responded that the substrate should not model dispatch at all: ship `LintEngine::run() -> Vec<Finding>` plus vocabulary; let engines own everything else. This revision captures that simplification. The substrate is now ~300 lines and stable under future viola ABI evolution.
