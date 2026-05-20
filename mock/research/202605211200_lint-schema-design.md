# Lint schema design (per-primitive configs, catalog, engine surfaces)

**Date:** 2026-05-21
**Status:** Schema lock. Concrete shapes for everything the consolidation proposal at `202605202200_lint-primitive-consolidation.md` (revision 2) deferred to this memo.
**Scope:** Locks the 17 items listed in the proposal's §"What the schema design memo locks". Input to Phase 2D implementation. Does not change the consolidation proposal; refines its decisions into concrete Rust types and TOML grammar.
**Sibling notes:**
- `mock/research/202605202200_lint-primitive-consolidation.md` (proposal, revision 2 at `3a0e02d`)
- `mock/research/202605202300_lint-primitive-proposal-review.md` (senior review of proposal)
- `mock/research/202605210000_lint-corpus-mechanism-audit.md` (per-lint mechanism catalog)
- `mock/research/202605201700_engine-preprocessor-architecture.md` (engine + preprocessor design)

## Why this note exists

The consolidation proposal settled the primitive set (11 reusable + 6 bespoke), the dispatch model (`Lint` trait with three modes and two methods), the scoping model (path filter + per-primitive visibility + per-gate staging), and the catalog mechanics (open-string kind + static registration). What it deferred to a follow-up memo are the concrete shapes: per-primitive `Config` types, the `MockspaceProject` / `MockspaceDocument` surfaces, `CatalogEntry` field-by-field, `ConfigError` and `LintError` shapes, `StagingFilter` API, glob syntax, test fixture grammar, CLI semantics, lint-pack registration, parallelism, editor surface.

This memo locks each. Implementation can start once this lands; nothing here is up for revision without an explicit follow-up memo.

## What this memo NOT locks

- The deletion-or-keep decision for the four "soft-bespoke" lints (`forbidden_imports`, `deprecation_comparison`, `design_doc_source_mismatch`, `registrable_completeness`): named below with the same options the proposal listed, plus a default pick where a default is reasonable. Implementation can revisit per-lint without changing the schema.
- The Viola plugin SDK shape: the consolidation proposal and this memo design the in-engine catalog. The viola transition is downstream; this catalog becomes the viola-plugin contract, but viola's plugin loader is its own design.
- The exact `tree_sitter` grammar versions: pinned per the engine-preprocessor architecture memo, not here.

## Section map

1. `Lint` trait surface (full, with the two-method shape).
2. `LintMode` and the per-mode staging semantics, locked.
3. `CatalogEntry` shape (all fields, with rationale).
4. Per-primitive `Config` schemas (11 reusable + 6 bespoke).
5. Unified scope schema (`[lints.<name>.scope]`).
6. Gate schema (`[lints.<name>.gate.<g>]`), per-finding-kind sub-blocks.
7. `MockspaceDocument` concrete-type contract.
8. `MockspaceProject` concrete-type contract.
9. `ConfigError` and `LintError` shapes.
10. `StagingFilter` API.
11. Override cascade and CLI semantics.
12. External lint-pack registration (`inventory` distributed slice).
13. Glob library and syntax.
14. Test fixture format.
15. Parallelism model.
16. Editor surface behaviour.
17. Suppression handoff.
18. What stays open (small list).

## 1. `Lint` trait

```rust
pub trait Lint: Send + Sync {
    /// Stable identifier; matches the `[lints.<name>]` TOML key. Unique within an
    /// instantiated catalog (config validation rejects duplicates).
    fn name(&self) -> &'static str;

    /// One-line human description; appears in diagnostic output and rendered docs.
    fn description(&self) -> &'static str;

    /// Default severity at each gate. Catalog default; consumer TOML overrides.
    fn default_severity(&self) -> GateSeverity;

    /// PerDocument mode dispatches here once per (filtered) document.
    fn check_document(
        &self,
        ctx: &LintContext<'_>,
        doc: &MockspaceDocument,
        sink: &mut dyn FindingSink,
    ) -> Result<(), LintError> {
        let _ = (ctx, doc, sink);
        unreachable!("check_document called on lint {} whose CatalogEntry::mode is not PerDocument; impl mismatch", self.name());
    }

    /// ProjectScoped and TwoPhaseProject modes dispatch here once per run.
    fn check_project(
        &self,
        ctx: &LintContext<'_>,
        project: &MockspaceProject,
        sink: &mut dyn FindingSink,
    ) -> Result<(), LintError> {
        let _ = (ctx, project, sink);
        unreachable!("check_project called on lint {} whose CatalogEntry::mode is PerDocument; impl mismatch", self.name());
    }

    /// Optional: whether this lint requires the syn AST cache to be warm.
    /// Engine pre-warms `MockspaceDocument::ast()` before dispatch when any
    /// active lint returns true.
    fn needs_syn_ast(&self) -> bool {
        false
    }

    /// Optional: tree-sitter cache pre-warm.
    fn needs_tree_sitter(&self) -> bool {
        false
    }
}
```

**Mode is read from `CatalogEntry::mode`, not from the `Lint` trait.** Earlier drafts carried a `Lint::mode()` method as well; that produced a silent-divergence footgun (the trait method and catalog field could disagree and the engine would route per the catalog while the impl body lived in the wrong method, producing an `unreachable!()` panic at first dispatch with no compile-time check). The fix is to make `CatalogEntry::mode` the single source of truth: the engine reads it once at instantiation and routes accordingly. A misconfigured catalog entry (mode says PerDocument but the impl only has `check_project`) still hits the `unreachable!()` on first dispatch, but the panic message names the catalog mismatch concretely.

Default impls of `check_document` / `check_project` are `unreachable!()`. The mismatch case is implementer error caught loudly at first dispatch, with a diagnostic naming the lint. Splitting into two traits (`PerDocumentLint` / `ProjectLint`) was considered; rejected because `Box<dyn Lint>` is the catalog storage shape and splitting forces an enum wrapper that pays its cost on every dispatch.

`finding_kinds` lives on `CatalogEntry` (not the trait): config validation rejects per-kind severity overrides for unknown kinds at instantiate. The trait does not redundantly declare them; the engine consults the catalog entry when validating emits.

`needs_syn_ast` / `needs_tree_sitter` are pre-warm hints. The engine walks active lints, OR-reduces these flags into a project-level pre-warm decision, then parallel-parses all relevant documents before dispatch. PerDocument lints never block on parse during dispatch.

`FindingSink::emit` enforces declared finding kinds in debug builds: the sink consults `CatalogEntry::finding_kinds` for the emitting lint and panics on unknown kinds in `debug_assertions`. Release builds skip the check. Cheap insurance against typo'd emit kinds.

## 2. `LintMode` and staging semantics

```rust
pub enum LintMode {
    /// Engine iterates project.documents() (or staged_documents() if scope-filtered),
    /// calls check_document for each.
    PerDocument,
    /// Engine calls check_project once. Lint reads engine state (e.g. SuppressionMap)
    /// but does not iterate documents itself.
    ProjectScoped,
    /// Engine calls check_project once. Lint walks project.documents() in Pass 1
    /// (collection) and project.documents() or project.staged_documents() in Pass 2
    /// (validation) per its own logic.
    TwoPhaseProject,
}
```

`staging_aware: bool` on `CatalogEntry` carries mode-dependent meaning:

| Mode | `staging_aware = true` | `staging_aware = false` |
|---|---|---|
| PerDocument | Engine pre-filters documents to staged set when `only_staged = true` | Engine always passes the full document set |
| TwoPhaseProject | Lint sees full project in `project.documents()`; staged subset in `project.staged_documents()` when `only_staged = true`. The lint chooses which to iterate per phase. | Both passes always walk the full project |
| ProjectScoped | INVALID; config validation rejects `staging_aware = true` here | The only legal value |

Config validation rule: `[lints.<name>.gate.<g>].only_staged = true` is accepted iff the lint's catalog entry has `staging_aware = true`. Engine never inspects `mode()` for staging semantics at dispatch time; the catalog flag is the source of truth.

Per-primitive catalog values (locked):

| Primitive | Mode | staging_aware |
|---|---|---|
| TokenScan | PerDocument | true |
| AstNodePositionMatch | PerDocument | true |
| AstTypePosition | PerDocument | true |
| IdentifierPattern | PerDocument | true |
| ContentRegex | PerDocument | true |
| TermReplacementTable | PerDocument | true |
| FileMetric | PerDocument | true |
| UndocumentedItem | PerDocument | true |
| CrossDocSymbolCheck | TwoPhaseProject | true |
| WorkflowState | TwoPhaseProject | false |
| SuppressionMeta | ProjectScoped | false |
| `no_bare_vec` (bespoke) | PerDocument | true |
| `no_manual_id` (bespoke) | PerDocument | true |
| `no_manual_impl` (bespoke) | PerDocument | true |
| `no_adhoc_framework` (bespoke) | TwoPhaseProject | true |
| `registrable_completeness` (bespoke) | TwoPhaseProject | false |
| `deprecation_comparison` (bespoke) | TwoPhaseProject | false |

Rationale spot-check: `WorkflowState` is `staging_aware = false` because design-round consistency (lock semantics, immutability, doc-gate transitions) needs the full design-rounds tree on every pass; validating only staged design-round files would miss inter-file invariants. `CrossDocSymbolCheck` is `staging_aware = true` because the collect phase always walks all documents (the symbol table is project-wide) and the validate phase can safely subset to staged docs.

## 3. `CatalogEntry`

```rust
pub struct CatalogEntry {
    /// Stable identifier; matches the `[lints.<name>]` TOML key.
    pub name: &'static str,

    /// One-line human description.
    pub description: &'static str,

    /// Open-string kind discriminator; selects the primitive impl.
    /// Built-in kinds:
    ///   "token-scan", "ast-node-position-match", "ast-type-position",
    ///   "identifier-pattern", "content-regex", "term-replacement-table",
    ///   "file-metric", "undocumented-item", "cross-doc-symbol",
    ///   "workflow-state", "suppression-meta".
    /// Bespoke lints register their own kind strings.
    pub kind: &'static str,

    /// TOML default config block; merged with consumer overrides per the cascade.
    pub default_config: &'static str,  // raw TOML, parsed at engine init

    /// TOML default scope block.
    pub default_scope: &'static str,

    /// Per-gate default severity.
    pub default_severity: GateSeverity,

    /// Optional default impact + category for diagnostic display.
    pub default_impact: Option<Impact>,
    pub default_category: Option<Category>,

    /// Optional URL into rendered docs; populates Finding::rule_id_url.
    pub doc_url: Option<&'static str>,

    pub mode: LintMode,
    pub staging_aware: bool,

    /// Whether this lint is skipped under RunSurface::Editor. Defaults to true
    /// for TwoPhaseProject and ProjectScoped modes (single-buffer LSP cannot
    /// supply a full project cheaply). PerDocument defaults to false (the lint
    /// runs on the currently-edited buffer with commit-gate severities).
    /// Consumers can override per-lint via TOML.
    pub editor_skip: bool,

    /// Constructor: validates the merged TOML and produces a Box<dyn Lint>.
    pub instantiate: fn(&toml::Table, &toml::Table) -> Result<Box<dyn Lint>, ConfigError>,

    /// Finding kinds this lint may emit. Drives per-kind severity validation.
    pub finding_kinds: &'static [&'static str],
}
```

`default_config` and `default_scope` are `&'static str` (raw TOML literal), not pre-parsed `toml::Table`. The engine parses both at startup. This keeps the catalog declaration `const`-friendly and avoids a global allocator dependency at static-init.

`instantiate` is a bare fn pointer. This is sufficient because per-pack defaults flow through `default_config` / `default_scope` strings; the constructor reads from the merged TOML. State a lint pack ships by default lives in the TOML, not in a closure capture. (Pattern documented for stack-lints authors: ship workspace-default tables as `&'static str` TOML literals, not as runtime-constructed maps.)

`finding_kinds` is the declared set of finding-kind strings the lint may emit. Empty slice means the lint emits one anonymous kind. Per-finding-kind severity overrides validate against this set.

## 4. Per-primitive Config schemas

Each primitive's `Config` type is the parsed shape of its `[lints.<name>.config]` TOML block. The `instantiate` function deserialises the merged TOML into the typed Config, validates it, and constructs the primitive.

### 4.1 TokenScan

```rust
pub struct TokenScanConfig {
    /// Tokens to scan for. Plain literal strings; no regex (use ContentRegex for regex).
    pub tokens: Vec<String>,

    /// Require word boundaries on both sides of each match.
    pub word_boundary: bool,

    /// Strip string literals before scanning.
    pub strip_strings: bool,

    /// Strip `//` and `/* */` comments.
    pub strip_comments: bool,

    /// Strip `///` and `//!` doc comments.
    pub strip_doc_comments: bool,

    /// Optional severity escalation when match count exceeds a threshold within a
    /// per-document context (legacy Pool B suppression-aware behaviour).
    pub severity_escalation: Option<EscalationRule>,
}

pub struct EscalationRule {
    pub threshold: USize,
    pub escalated_severity: Severity,
}
```

Defaults: `word_boundary = true`, `strip_strings = true`, `strip_comments = true`, `strip_doc_comments = true`, `severity_escalation = None`.

```toml
[lints.no-alloc.config]
tokens = ["Vec<", "String", "Box<", "Rc<", "Arc<", "vec!", "HashMap<", "BTreeMap<"]
word_boundary = true
strip_strings = true
strip_comments = true
strip_doc_comments = true
```

### 4.2 AstNodePositionMatch

```rust
pub struct AstNodePositionConfig {
    /// Tree-sitter node kinds to walk.
    pub node_kinds: Vec<TsNodeKind>,

    /// For `macro_invocation` nodes: macro names to fire on.
    pub macro_names: Option<Vec<String>>,

    /// For `impl_item` nodes: trait names to fire on.
    pub trait_names: Option<Vec<String>>,

    /// For `call_expression` / `field_expression` nodes: method/field names.
    pub member_names: Option<Vec<String>>,

    /// Ancestor node kinds whose subtrees are skipped (e.g. macro_definition for
    /// "fire only on macro invocations outside macro definitions").
    pub exclude_under: Vec<TsNodeKind>,
}

pub enum TsNodeKind {
    MacroInvocation,
    MacroDefinition,
    EnumItem,
    StructItem,
    ImplItem,
    FunctionItem,
    CallExpression,
    FieldExpression,
    UseDeclaration,
    AttributeItem,
}
```

The `TsNodeKind` enum is closed (mockspace-rs only walks these). Adding a new node kind is a code edit in mockspace-rs; not a TOML edit. This is intentional: tree-sitter node-kind strings are grammar-version-sensitive, and a closed enum is the correct boundary.

```toml
[lints.no-todo.config]
node_kinds = ["macro-invocation"]
macro_names = ["todo", "unimplemented", "panic"]
exclude_under = ["macro-definition"]
```

### 4.3 AstTypePosition

```rust
pub struct AstTypePositionConfig {
    /// Type names to fire on.
    pub forbidden_types: Vec<String>,

    /// Type-bearing positions to inspect.
    pub positions: Vec<TypePosition>,

    /// Visibility filter. Per-primitive, not per-scope.
    pub visibility: Visibility,

    /// Optional category-based exemption from `[primitive-introductions]`.
    /// Documents whose crate has any of these categories are skipped.
    pub exempt_categories: Vec<Category>,

    /// Optional per-type replacement suggestion (for SemanticAliasNudge-shaped lints).
    /// If present, the Finding carries a FixSuggestion with the replacement text.
    pub replacements: Vec<(String, String)>,

    /// Severity escalation by match count within a document.
    pub severity_escalation: Option<EscalationRule>,
}

pub enum TypePosition {
    StructField,
    EnumVariantField,
    FnParam,
    FnReturn,
    TypeAliasBody,
    AssociatedType,
}

pub enum Visibility {
    Any,
    Public,
}
```

Visibility lives on this Config (and on every primitive Config that accepts it), not on scope. Setting `scope.visibility` is a `ConfigError::UnknownField`.

`replacements` is `Vec<(String, String)>` (not `HashMap`) so order is stable. Look-up is O(n) per match; for the expected list sizes (under 50) this is irrelevant.

```toml
[lints.no-bare-string.config]
forbidden_types = ["String", "&str"]
positions = ["struct-field", "fn-param", "fn-return"]
visibility = "public"
exempt_categories = ["string-foundation"]
strip_strings = true
```

### 4.4 IdentifierPattern

```rust
pub struct IdentifierPatternConfig {
    /// Item kinds to walk.
    pub item_kinds: Vec<ItemKind>,

    /// Forbidden prefix match (case-sensitive).
    pub forbidden_prefixes: Vec<String>,

    /// Forbidden suffix match (case-sensitive).
    pub forbidden_suffixes: Vec<String>,

    /// Forbidden regex match. Pre-compiled at instantiate; ConfigError on bad regex.
    pub forbidden_regexes: Vec<String>,

    /// Visibility filter.
    pub visibility: Visibility,
}

pub enum ItemKind {
    Struct,
    Enum,
    Fn,
    Trait,
    TypeAlias,
    Const,
    Static,
    Mod,
}
```

Regex syntax: the `regex` crate's default flavour. ConfigError on un-compileable patterns at instantiate, not at first dispatch.

### 4.5 ContentRegex

```rust
pub struct ContentRegexConfig {
    pub patterns: Vec<ContentPattern>,
}

pub struct ContentPattern {
    pub regex: String,
    pub message: String,
    pub finding_kind: String,  // must appear in CatalogEntry::finding_kinds
    pub ratio: Option<RatioThreshold>,  // None = fire on every match
    pub strip_code_fences: bool,
}

pub struct RatioThreshold {
    /// Maximum matches per `lines_window` lines tolerated before firing.
    pub max_matches: USize,
    pub lines_window: USize,
}
```

Each pattern is independent; the lint emits one finding per match per pattern unless the ratio threshold is set, in which case the lint emits one finding per window-exceedence.

```toml
[lints.writing-style.config]

[[lints.writing-style.config.patterns]]
regex = "—"
message = "Em-dashes are forbidden; use period, comma, or parens."
finding_kind = "em-dash"
strip_code_fences = true

[[lints.writing-style.config.patterns]]
regex = '\b(leverage|seamless|robust|powerful|holistic)\b'
message = "Marketing word; rewrite to describe the concrete property."
finding_kind = "marketing-word"
strip_code_fences = true
```

### 4.6 TermReplacementTable

```rust
pub struct TermReplacementTableConfig {
    /// Map of dead term -> canonical replacement.
    pub replacements: Vec<(String, String)>,

    /// Require word boundaries on both sides.
    pub word_boundary: bool,

    pub strip_strings: bool,
    pub strip_comments: bool,
    pub strip_doc_comments: bool,
}
```

Defaults: `word_boundary = true`, `strip_strings = true`, `strip_comments = false` (vocabulary applies to comments too), `strip_doc_comments = false`.

```toml
[lints.vocabulary-discipline.config]
word_boundary = true
strip_strings = true
strip_comments = false

[lints.vocabulary-discipline.config.replacements]
"substrate" = "foundations"
"HList" = "cons-list"
"chain" = "fiber"
"partition" = "phase"
"entity" = "record"
```

Each match produces a Finding whose message names the matched term plus the canonical replacement, plus a FixSuggestion replacing the matched text with the canonical form.

### 4.7 FileMetric

```rust
pub struct FileMetricConfig {
    pub metric: Metric,
    pub threshold: USize,
    pub inclusive: bool,  // true = >=, false = >
}

pub enum Metric {
    LineCount,
    NonBlankLineCount,
    NonBlankNonCommentLineCount,
    PubItemCount,
    PrivateItemCount,
    TotalItemCount,
}
```

### 4.8 UndocumentedItem

```rust
pub struct UndocumentedItemConfig {
    pub item_kinds: Vec<ItemKind>,
    pub visibility: Visibility,
    pub shame_escape: Option<ShameEscapeRule>,
}

pub struct ShameEscapeRule {
    /// Minimum rationale length (in words) for a SHAME entry to count as a valid escape.
    pub min_words: USize,
    /// Path relative to crate root.
    pub shame_path: PathBuf,
}
```

If `shame_escape` is set, the lint checks the SHAME file for an entry whose rationale word-count exceeds `min_words` keyed by the item's name. A matching entry suppresses the finding.

### 4.9 CrossDocSymbolCheck

```rust
pub struct CrossDocSymbolCheckConfig {
    pub symbol_kind: SymbolKind,
    pub visibility: Visibility,
    pub predicate: CrossDocPredicate,
}

pub enum SymbolKind {
    Fn,
    Type,
    Trait,
    Const,
    Mod,
}

pub enum CrossDocPredicate {
    /// For each symbol in the configured kind set, fire if it appears in
    /// multiple crates with `pub` visibility.
    NoDuplicatesAcrossCrates,

    /// For each pub symbol in source, fire if it does NOT appear backticked
    /// in any document matching `design_doc_glob`.
    /// (Direction: source -> doc; "every pub item must be documented".)
    SourceMustAppearInDoc { design_doc_glob: String },

    /// For each backticked symbol name in documents matching `design_doc_glob`,
    /// fire if no matching pub item exists in source.
    /// (Direction: doc -> source; "every documented item must exist".)
    /// This is the design_doc_source_mismatch failure mode the per-lint audit
    /// flagged — backticked claims about source state that have rotted.
    DocMustReferenceSource { design_doc_glob: String },

    /// For each symbol in source, fire if it does not match the corresponding
    /// entry in the deprecated-CLs directory (covers deprecation_comparison
    /// scaffolding; see §18 for the bespoke-vs-absorb decision on that lint).
    MustMatchDeprecationEntry { deprecated_cls_dir: PathBuf },

    /// Catch-all: for each pub symbol, fire if the documents matching `doc_glob`
    /// do not contain `ref_pattern` (a literal substring or a regex if prefixed
    /// with `re:`).
    MustBeReferencedInDoc { doc_glob: String, ref_pattern: String },
}
```

`design_doc_glob` is a glob relative to project root (e.g. `mock/crates/*/DESIGN.md.tmpl`). The two-direction predicates (`SourceMustAppearInDoc` + `DocMustReferenceSource`) ship as separate enum variants rather than one variant with a direction flag because validation behaviour differs: the source -> doc walk iterates source pub items in Pass 2; the doc -> source walk iterates the backticked-symbol set collected from docs in Pass 1, against the symbol table built from source. Each emits different finding kinds.

### 4.10 WorkflowState

```rust
pub struct WorkflowStateConfig {
    pub rule: WorkflowRule,
}

pub enum WorkflowRule {
    /// Locked CLs cannot be edited after lock.
    ChangelistLock,

    /// CL files conform to the state-machine naming convention.
    ChangelistImmutability,

    /// Every design round has a doc CL.
    ChangelistRequired,

    /// Doc CL is locked before src CL can lock.
    ChangelistDocGate,

    /// Files in `mock/design_rounds/` match the YYYYMMDDHHMM_ pattern.
    DesignRoundFilenameConvention,
}
```

Each rule is one lint instance in the catalog; the consumer enables / configures each independently.

### 4.11 SuppressionMeta

```rust
pub struct SuppressionMetaConfig {
    pub require_tracked: bool,
    pub require_reason: bool,
    pub require_reason_min_words: USize,
    pub forbid_expired: bool,
    pub overuse_threshold_per_crate: Option<USize>,
}
```

`require_tracked = true` requires every `lint:allow(...)` to carry `tracked: #N`. `forbid_expired = true` requires the referenced task to be open (engine reads task state via `MockspaceProject::workspace()`). `overuse_threshold_per_crate` fires one finding per crate that exceeds the count.

### 4.12 `no_bare_vec` (bespoke)

```rust
pub struct NoBareVecConfig {
    /// Phase 1: AST walk over type-bearing positions.
    pub forbidden_types: Vec<String>,
    pub positions: Vec<TypePosition>,
    pub visibility: Visibility,

    /// Phase 2: text scan inside define_*! macro bodies.
    pub macro_body_tokens: Vec<String>,
    pub macros: Vec<String>,  // e.g. ["define_resource", "define_column"]

    /// When a macro invocation contains another macro invocation in its token
    /// stream (e.g. `define_resource! { initial = vec![...] }`), recurse into
    /// the inner invocation's tokens. Default true; guards loops via max_depth.
    pub recurse_into_nested_macros: bool,

    /// Maximum recursion depth when traversing nested macros. Default 8.
    pub max_recursion_depth: USize,
}
```

Phase 1 mirrors `AstTypePosition`; Phase 2 scans inside specific macro invocations for tokens, recursing into nested invocations up to `max_recursion_depth`.

### 4.13 `no_manual_id` (bespoke)

```rust
pub struct NoManualIdConfig {
    /// Detect `struct X(Y);` newtype patterns where Y is a primitive integer.
    pub primitive_inner_types: Vec<String>,
    /// Type alias detection: `type X = Y;` where Y is a primitive integer.
    pub check_aliases: bool,
}
```

### 4.14 `no_manual_impl` (bespoke)

```rust
pub struct NoManualImplConfig {
    /// Trait names that should be derived, not hand-implemented.
    pub forbidden_traits: Vec<String>,
}
```

Default: `["Clone", "Copy", "Debug", "Default", "PartialEq", "Eq", "Hash"]`.

### 4.15 `no_adhoc_framework` (bespoke)

```rust
pub struct NoAdhocFrameworkConfig {
    /// Detect dispatch-table patterns (struct with fn-pointer fields + central dispatcher).
    pub detect_dispatch_tables: bool,
    /// Detect init/run/cleanup triples.
    pub detect_lifecycle_triples: bool,
    /// Detect callback-chain patterns.
    pub detect_callback_chains: bool,
    /// Minimum match-count to fire (heuristic threshold).
    pub min_signal_count: USize,
}
```

Heuristic. Lives in the bespoke bucket precisely because tuning is a one-off per consumer.

### 4.16 `registrable_completeness` (bespoke)

```rust
pub struct RegistrableCompletenessConfig {
    /// Trait whose impls must be complete.
    pub trait_name: String,
    /// Required associated items (fns, types, consts).
    pub required_items: Vec<RequiredItem>,
}

pub struct RequiredItem {
    pub name: String,
    pub kind: ItemKind,
    pub min_signature_complexity: USize,  // word-count heuristic
}
```

### 4.17 `deprecation_comparison` (bespoke)

```rust
pub struct DeprecationComparisonConfig {
    /// Glob for active CL files.
    pub active_cls_glob: String,
    /// Glob for deprecated CL files.
    pub deprecated_cls_glob: String,
    /// Symbol kinds to compare.
    pub symbol_kinds: Vec<SymbolKind>,
}
```

Reports symbols present in deprecated CLs but absent from active CLs (forgotten removals) or vice versa (orphan additions).

## 5. Unified scope schema

```rust
pub struct ScopeConfig {
    /// File-system globs for files this lint sees.
    pub paths: Vec<String>,

    /// File-system globs to exempt.
    pub exempt_paths: Vec<String>,

    /// Crate name patterns. `*` = all crates.
    pub crates: Vec<String>,

    /// Crate name patterns to exempt.
    pub exempt_crates: Vec<String>,

    /// Language filter (Rust, Markdown).
    pub languages: Vec<Language>,

    /// Categories declared in `[primitive-introductions]` whose owning crates
    /// are exempted from this lint.
    pub exempt_categories: Vec<Category>,

    /// Exempt all crates listed in workspace `proc_macro_crates`.
    pub proc_macro_exempt: bool,
}
```

**No `visibility` field on scope.** Visibility lives on per-primitive Config. Setting `scope.visibility` is `ConfigError::UnknownField`.

```toml
[lints.no-bare-vec.scope]
paths = ["**/*.rs"]
exempt_paths = ["**/ffi/**", "**/tests/**", "**/benches/**"]
crates = ["*"]
exempt_crates = ["arvo-storage"]
languages = ["rust"]
exempt_categories = ["bare-collection-foundation"]
proc_macro_exempt = true
```

Glob syntax is `globset`'s; see §13 for the full grammar.

## 6. Gate schema

```rust
pub struct GateSeverity {
    pub commit: GateConfig,
    pub build: GateConfig,
    pub push: GateConfig,
}

pub struct GateConfig {
    pub severity: Severity,
    pub only_staged: bool,
    pub skip: bool,
    pub finding_kinds: Option<HashMap<String, Severity>>,
}
```

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

# Per-finding-kind override
[lints.writing-style.gate.commit]
severity = "warn"

[lints.writing-style.gate.commit.finding_kinds]
"em-dash" = "error"
"marketing-word" = "warn"
"filler" = "hint"
```

Config validation rules:

- `only_staged = true` accepted iff `CatalogEntry::staging_aware = true`.
- Keys under `finding_kinds` must appear in `CatalogEntry::finding_kinds`; unknown keys produce `ConfigError::UnknownFindingKind`.
- `skip = true` drops the lint at that gate regardless of severity. Useful for "writing-style only at push".

## 7. `MockspaceDocument`

```rust
pub struct MockspaceDocument {
    path: PathBuf,
    crate_name: String,
    language: Language,
    source: String,
    content_hash: ContentHash,

    syn_ast_cache: OnceCell<Option<syn::File>>,
    tree_sitter_cache: OnceCell<Option<tree_sitter::Tree>>,
    source_stripped_cache: RwLock<HashMap<StripOpts, Arc<str>>>,
}

impl MockspaceDocument {
    pub fn path(&self) -> &Path { &self.path }
    pub fn crate_name(&self) -> &str { &self.crate_name }
    pub fn language(&self) -> Language { self.language }
    pub fn source(&self) -> &str { &self.source }
    pub fn content_hash(&self) -> ContentHash { self.content_hash }

    pub fn ast(&self) -> Option<&syn::File> {
        self.syn_ast_cache
            .get_or_init(|| syn::parse_file(&self.source).ok())
            .as_ref()
    }

    pub fn tree_sitter(&self) -> Option<&tree_sitter::Tree> {
        self.tree_sitter_cache
            .get_or_init(|| parse_with_tree_sitter(&self.source, self.language))
            .as_ref()
    }

    pub fn source_stripped(&self, opts: StripOpts) -> Arc<str> {
        // Cached per (document, opts) pair. Cache holds Arc<str> directly;
        // callers receive a cheap clone of the Arc, not a fresh allocation.
        let read = self.source_stripped_cache.read();
        if let Some(arc) = read.get(&opts) { return Arc::clone(arc); }
        drop(read);

        let stripped: Arc<str> = Arc::from(strip(&self.source, opts));
        let mut write = self.source_stripped_cache.write();
        let arc = write.entry(opts).or_insert_with(|| Arc::clone(&stripped));
        Arc::clone(arc)
    }
}

#[derive(Hash, Eq, PartialEq, Copy, Clone)]
pub struct StripOpts {
    pub strings: bool,
    pub comments: bool,
    pub doc_comments: bool,
    pub code_fences: bool,  // for markdown
}
```

The cache is `OnceCell` for AST (single computation, no concurrent rebuild) and `RwLock<HashMap>` for source-stripped views (per-opts variance). Returning `Arc<str>` from `source_stripped` lets the caller hold the view without keeping a `RwLockReadGuard` live.

Note `MockspaceDocument` lives in mockspace-rs, not mockspace-core. The foundation `Document` trait (4 methods) is implemented by `MockspaceDocument` for compatibility with engine-internal foundation surfaces; the AST methods are concrete-type-only.

## 8. `MockspaceProject`

```rust
pub struct MockspaceProject {
    documents: Vec<MockspaceDocument>,
    staged_indices: Vec<usize>,  // indices into documents
    crate_graph: CrateGraph,
    workspace: WorkspaceMetadata,
    design_rounds: DesignRoundsView,
    suppressions: SuppressionMap,
    surface: RunSurface,
    gate: Gate,
    introduced_categories: HashMap<String, CategorySet>,  // crate name -> categories
}

impl MockspaceProject {
    pub fn documents(&self) -> impl Iterator<Item = &MockspaceDocument> {
        self.documents.iter()
    }

    pub fn staged_documents(&self) -> impl Iterator<Item = &MockspaceDocument> {
        self.staged_indices.iter().map(|&i| &self.documents[i])
    }

    // Project construction: in RunSurface::Editor, the project carries one
    // document (the currently-edited buffer) and `staged_indices = [0..1]` —
    // the buffer counts as staged so PerDocument lints with only_staged = true
    // actually see it. This preserves commit-gate parity for editor-time
    // diagnostics.

    pub fn crate_graph(&self) -> &CrateGraph { &self.crate_graph }
    pub fn workspace(&self) -> &WorkspaceMetadata { &self.workspace }
    pub fn design_rounds(&self) -> &DesignRoundsView { &self.design_rounds }
    pub fn suppressions(&self) -> &SuppressionMap { &self.suppressions }
    pub fn surface(&self) -> RunSurface { self.surface }
    pub fn gate(&self) -> Gate { self.gate }

    pub fn introduced_categories(&self, crate_name: &str) -> &CategorySet {
        self.introduced_categories
            .get(crate_name)
            .unwrap_or(&CategorySet::EMPTY)
    }
}

pub struct CrateGraph {
    /// All crates in the project (from Cargo metadata).
    crates: Vec<CrateInfo>,
    /// Map from crate name to index.
    by_name: HashMap<String, usize>,
}

pub struct CrateInfo {
    pub name: String,
    pub root_path: PathBuf,
    pub is_proc_macro: bool,
    pub is_workspace_member: bool,
    pub deps: Vec<String>,  // crate names
}

pub struct WorkspaceMetadata {
    pub root: PathBuf,
    pub proc_macro_crates: HashSet<String>,
    pub task_state: TaskStateView,  // for SuppressionMeta forbid_expired
}

pub struct DesignRoundsView {
    /// Path to mock/design_rounds/ root.
    pub root: PathBuf,
    /// Discovered rounds (parsed at project load).
    pub rounds: Vec<DesignRound>,
}

pub struct DesignRound {
    pub timestamp: String,
    pub state: RoundState,  // Topic, Doc, Src, Locked, Closed
    pub doc_cl: Option<PathBuf>,
    pub src_cl: Option<PathBuf>,
    pub locked: bool,
}
```

`CrateGraph` is built once at project load from `cargo metadata` output. Scope pre-filtering on `crates` / `exempt_crates` / `proc_macro_exempt` uses it. `crate_graph().is_proc_macro(name)` answers the proc-macro question without re-scanning Cargo.toml.

`DesignRoundsView` is built once at project load by walking `mock/design_rounds/`. `WorkflowState` consumes it. `MockspaceProject::workspace().task_state` is queried by `SuppressionMeta` when `forbid_expired = true`.

## 9. `ConfigError` and `LintError`

```rust
pub struct ConfigError {
    pub lint_name: String,
    pub field_path: String,
    pub kind: ConfigErrorKind,
    pub message: String,
    pub source_location: Option<Span>,  // into lints.toml when known
}

pub enum ConfigErrorKind {
    UnknownField,
    TypeMismatch { expected: &'static str, actual: &'static str },
    InvalidValue,
    ContradictsCatalog,
    UnknownKind,
    UnknownFindingKind,
    UnparseableRegex { error: String },
    UnparseableGlob { error: String },
    Duplicate,
}

pub enum LintError {
    /// Source parse failure on a document the lint required.
    ParseFailure { path: PathBuf, parser: &'static str, source: String },
    /// Internal invariant violation in the lint impl.
    Internal(String),
    /// I/O failure reaching workflow state.
    WorkflowIo(io::Error),
    /// Catalog config mismatch detected at dispatch time (should have been caught at instantiate).
    LateConfigError(ConfigError),
}
```

Engine returns `Outcome<Vec<Box<dyn Lint>>, Vec<ConfigError>>` from instantiation. CI fails on the error vec; the formatter renders config errors against their `lints.toml` source location when available. `LintError` flows through the dispatch loop: the engine catches per-lint, converts to a synthetic `Finding` tagged with the lint name and `LintError` kind, and the run continues with the remaining lints.

The two channels stay separate. `Finding`s are about source code; `ConfigError`s are about configuration. Mixing them pollutes both diagnostic types.

## 10. `StagingFilter`

```rust
pub struct StagingFilter {
    gate: Gate,
    base_ref: Option<String>,
    staged_paths: HashSet<PathBuf>,
}

impl StagingFilter {
    /// Build per the current gate. Reads git via std::process::Command.
    /// Returns Err on misconfigured environment (e.g. MOCKSPACE_PUSH_DIFF_BASE
    /// names a non-existent ref); the engine surfaces this as a ConfigError
    /// to the run-output channel and refuses to dispatch staging-aware lints
    /// rather than silently treating "0 staged files" as "drop everything".
    pub fn new(gate: Gate, workspace_root: &Path) -> Result<Self, StagingFilterError> {
        let staged = match gate {
            Gate::Commit => Self::staged_for_commit(workspace_root)?,
            Gate::Push => Self::staged_for_push(workspace_root)?,
            Gate::Build => StagedSet::Full,  // build gate sees everything
        };
        Ok(Self { gate, base_ref: None, staged })
    }

    fn staged_for_commit(root: &Path) -> Result<StagedSet, StagingFilterError> {
        run_git(root, &["diff", "--name-only", "--cached"])
            .map(StagedSet::Paths)
            .map_err(StagingFilterError::Git)
    }

    fn staged_for_push(root: &Path) -> Result<StagedSet, StagingFilterError> {
        // Resolution: env override > @{upstream} > full-project fallback (warned).
        // Env override errors loudly when the ref does not resolve; the fallback
        // chain is only consulted when the env var is unset.
        if let Ok(base) = env::var("MOCKSPACE_PUSH_DIFF_BASE") {
            if !git_rev_parse_verify(root, &base) {
                return Err(StagingFilterError::BadEnvRef { value: base });
            }
            return run_git(root, &["diff", "--name-only", &format!("{base}..HEAD")])
                .map(StagedSet::Paths)
                .map_err(StagingFilterError::Git);
        }
        if let Ok(upstream) = git_rev_parse_upstream(root) {
            let base = git_merge_base(root, "HEAD", &upstream).unwrap_or(upstream);
            return run_git(root, &["diff", "--name-only", &format!("{base}..HEAD")])
                .map(StagedSet::Paths)
                .map_err(StagingFilterError::Git);
        }
        // Detached HEAD with no upstream and no env: full project scan with a warning.
        eprintln!(
            "warning: push gate falling back to full project; \
             set MOCKSPACE_PUSH_DIFF_BASE to gate against a specific ref"
        );
        Ok(StagedSet::Full)
    }

    pub fn is_staged(&self, path: &Path) -> bool {
        match &self.staged {
            StagedSet::Full => true,
            StagedSet::Paths(set) => set.contains(path),
        }
    }
}

pub enum StagedSet {
    /// All documents count as staged (build gate, push-gate fallback).
    Full,
    /// Only listed paths.
    Paths(HashSet<PathBuf>),
}

pub enum StagingFilterError {
    /// MOCKSPACE_PUSH_DIFF_BASE was set but does not resolve to a git object.
    BadEnvRef { value: String },
    /// git command failed.
    Git(GitError),
}
```

`run_git` returns `Result<HashSet<PathBuf>, GitError>`; non-zero exit, missing git binary, and parse failures all surface as errors. The engine catches `StagingFilterError` at construction and reports it as a ConfigError-channel item; staging-aware lints are dropped from the active set with a clear diagnostic. No silent "0 files staged → everything drops" path. Output parsing handles empty lines and non-UTF-8 paths (the latter skipped with a per-path warning).

## 11. Override cascade and CLI semantics

Cascade order (lowest precedence to highest):

1. `CatalogEntry::default_config` + `default_scope` + `default_severity`.
2. Workspace-level `[lints]` defaults (the `default_severity` field).
3. Per-lint `[lints.<name>]` blocks in `lints.toml`.
4. CLI overrides (`--scope`, `--lint`, `--severity-override`, future `--fix`).

CLI semantics:

- `--scope <crate>`: **intersects** with each active lint's `scope.crates`. Lints with empty resulting intersection are dropped silently (a verbose-log line explains why). No override semantics.
- `--lint <name>`: filters the active set to only the named lint(s). Multiple invocations or comma-separated.
- `--severity-override <lint>=<sev>`: bumps the lint's effective severity at the current gate to `<sev>`. Engine validates the lint exists.
- `--fix`: future hook. The flag is reserved but not consumed by the catalog instantiate path; the runner consults it at finding emit.

## 12. External lint-pack registration

Stack-lints (and future lint packs) contribute catalog entries via the `inventory` crate's distributed slice:

```rust
// In mockspace-rs:
inventory::collect!(CatalogEntry);

// In stack-lints:
inventory::submit! {
    CatalogEntry {
        name: "strategy-marker-required",
        kind: "ast-type-position",
        // ... full entry
        instantiate: |config, scope| { /* ... */ },
        // ...
    }
}
```

Engine startup:

```rust
let entries: Vec<CatalogEntry> = inventory::iter::<CatalogEntry>().cloned().collect();
let mut by_name: HashMap<&'static str, &CatalogEntry> = HashMap::new();
for entry in &entries {
    if by_name.insert(entry.name, entry).is_some() {
        return Err(ConfigError {
            lint_name: entry.name.to_string(),
            kind: ConfigErrorKind::Duplicate,
            // ...
        });
    }
}
```

Build-time only. Consumer Cargo.toml lists stack-lints (or any future lint pack) as a dep; rebuilds pick up new entries. No dynamic loading.

`inventory` works under `#![no_std]` for the distributed-slice mechanism; `mockspace-rs` is `std`-capable anyway (the engine binary, not the foundations).

## 13. Glob library and syntax

Library: `globset` (vendored: no `regex-syntax` newfangled features).

Syntax:

- `*` matches any characters except `/`.
- `**` matches any number of path components.
- `?` matches a single character except `/`.
- `[abc]` character classes.
- `{a,b,c}` brace expansion.
- `!pattern` negation (only at the start of a single-glob string).

Examples:

```
**/*.rs                 // all Rust files at any depth
mock/crates/*/src/**/*.rs  // any Rust file under any crate's src/
!target/**              // negation (used in exempt_paths)
mock/crates/{arvo,arvo-bits}/**  // brace expansion
```

Glob compilation happens at instantiate; `ConfigError::UnparseableGlob` on syntax error. Compiled `GlobSet`s are reused across documents.

## 14. Test fixture format

Each primitive ships a fixture-driven test suite. Format: TOML.

```toml
# fixtures/token-scan/no-alloc-positive.toml

[fixture]
description = "Vec<u8> in pub fn signature should fire"

[fixture.config]
tokens = ["Vec<"]
word_boundary = true

[fixture.scope]
paths = ["**/*.rs"]

[fixture.source]
path = "lib.rs"
content = '''
pub fn foo() -> Vec<u8> {
    vec![]
}
'''

[[fixture.expected]]
finding_kind = ""
line = 1
column = 17
severity = "warn"
message_contains = "Vec<"
```

The test runner loads each `.toml` file, instantiates the primitive with the embedded config, runs `check_document` or `check_project` over the embedded source, and asserts the emitted findings match the `[[fixture.expected]]` array.

Each primitive ships at minimum: positive case, negative case (no findings), exemption case, scope variation case. Bespoke primitives ship at least three fixtures each.

## 15. Parallelism model

- **PerDocument lints**: per-document parallelism via `rayon`. Each (document, lint) pair runs as an independent task. The `FindingSink` is thread-safe (`Arc<Mutex<Vec<Finding>>>` internally).
- **TwoPhaseProject lints**: serial. The lint owns its Pass 1 / Pass 2 logic; internal parallelism is the lint's choice.
- **ProjectScoped lints**: serial. By definition, one dispatch per run.

Default rayon thread pool sized to `num_cpus::get()`. Override via `MOCKSPACE_LINT_THREADS` env var.

The `FindingSink` trait is the synchronisation boundary:

```rust
pub trait FindingSink: Send + Sync {
    fn emit(&self, finding: Finding);
}
```

Implementations:
- `VecFindingSink(Mutex<Vec<Finding>>)`: collects in-process.
- `StreamingFindingSink(crossbeam_channel::Sender<Finding>)`: streams to a consumer thread.

## 16. Editor surface

`RunSurface::Editor` bypasses the staging filter entirely. The editor invokes the engine with a single `MockspaceDocument` (the currently-edited buffer); the engine constructs a `MockspaceProject` containing only that document; all PerDocument lints run with commit-gate severities.

TwoPhase and ProjectScoped lints **skip** in Editor surface by default. Rationale: Pass 1 of TwoPhase wants the full project to build the symbol table, which the editor cannot supply efficiently for every keystroke. `CatalogEntry::editor_skip: bool` defaults to `true` for non-PerDocument modes; consumers can override via per-lint TOML if they have a fast project-load story.

```rust
pub enum RunSurface {
    /// CLI invocation (`cargo mock lint`, `mockspace lint`).
    Local,
    /// CI invocation; severities ramp to error per gate.
    Ci,
    /// LSP / editor invocation; single-document scope, commit-gate severities.
    Editor,
}
```

## 17. Suppression handoff

The engine populates `MockspaceProject::suppressions` once at project load by walking all documents for `lint:allow(...)` annotations (via the preprocessor in `mockspace-core::lint::SuppressionMap`). Lints never read source for suppression themselves.

Dispatch flow:

1. Lint emits Findings via the sink.
2. After the lint returns, the engine filters the sink's collected findings against the SuppressionMap: any finding whose span is inside an enclosing `#[mock::lints::allow(<lint-name>)]` (or equivalent) scope is dropped.
3. Surviving findings flow to the output formatter.

Primitive 11 (`SuppressionMeta`) is the only lint that reads `suppressions()` directly. It validates the map's contents (tracked: #N presence, reason length, etc.) but does not modify the map.

## 18. Open items deferred to implementation

These are decisions the schema does not force but that surface during Phase 2D coding. Implementer's call, with the listed options:

1. **`forbidden_imports` shape.** Options: ship as multiple TokenScan instances with a TOML helper that expands `[lints.forbidden-imports.rules.<rule_name>]` into discrete catalog entries at engine load, or keep as a single bespoke primitive with its own multi-rule Config. **Default pick:** multi-TokenScan with helper. Cleaner catalog; rules show up as discrete entries.

2. **`design_doc_source_mismatch` shape.** Decided in §4.9: ships as `CrossDocSymbolCheck` with the new `DocMustReferenceSource` predicate variant. The SHAME-entry escape lives in `UndocumentedItemConfig`'s `shame_escape` shape, generalised. The reverse-direction predicate (`SourceMustAppearInDoc`) covers the source→doc validation. Two predicate variants, no bespoke primitive.

3. **`deprecation_comparison` mode.** Options: bespoke as currently scoped, or absorb into `WorkflowState` as a fifth rule. **Default pick:** bespoke. The cross-CL diff logic is meaningfully different from typestate validation.

4. **Tree-sitter grammar versions.** Locked per the engine-preprocessor architecture memo (`tree-sitter-rust = "0.20"`). Schema does not redocument here.

5. **`SemanticAliasNudge` extension shape.** Options: `AstTypePosition` with `replacements: Vec<(String, String)>` field (locked here at §4.3), or separate `TypePositionReplacement` primitive. **Pick:** field on `AstTypePosition`. Symmetry with `TermReplacementTable` (content-side replacement table).

## 19. References

- Proposal: `mock/research/202605202200_lint-primitive-consolidation.md` (revision 2 at `3a0e02d`).
- Senior review: `mock/research/202605202300_lint-primitive-proposal-review.md`.
- Per-lint mechanism audit: `mock/research/202605210000_lint-corpus-mechanism-audit.md`.
- Engine + preprocessor architecture: `mock/research/202605201700_engine-preprocessor-architecture.md` (the foundation crate contract this memo respects).
- Workspace rule on toolbox-not-policer: `~/Dev/clause-dev/.claude/rules/arvo-toolbox-not-policer.md`.
- Workspace vocabulary rule: `~/Dev/clause-dev/.claude/rules/vocabulary.md`.

## 20. Recorded

2026-05-21. Authored after the consolidation proposal cleared second-pass senior review (schema-memo-ready verdict). This memo locks the per-primitive Config shapes, the engine surfaces (`MockspaceDocument`, `MockspaceProject`, `CatalogEntry`, `ConfigError`, `LintError`, `StagingFilter`), and the operational mechanics (override cascade, CLI semantics, lint-pack registration, glob syntax, test fixture format, parallelism, editor surface, suppression handoff).

**2026-05-21 (memo revision 2).** Senior review of the first memo draft caught six items:

1. `Lint::mode()` + `CatalogEntry::mode` duplication. Resolution: `Lint::mode()` dropped from the trait; `CatalogEntry::mode` is the single source of truth.
2. `CatalogEntry::editor_skip` referenced in §16 but missing from the struct in §3. Resolution: field added.
3. `MockspaceProject::staged_documents()` semantics in Editor surface unspecified. Resolution: locked at "all documents count as staged" with commit-gate severities; commit-gate parity preserved.
4. `StagingFilter` silent fallback on bad `MOCKSPACE_PUSH_DIFF_BASE`. Resolution: `StagingFilter::new` returns `Result`; bad ref errors loudly via `StagingFilterError::BadEnvRef`; engine drops staging-aware lints with a ConfigError-channel diagnostic.
5. `Arc<str>` cache cloning defeated the source-stripped cache. Resolution: cache stores `Arc<str>` directly; callers receive `Arc::clone` not fresh allocations.
6. `CrossDocSymbolCheck` predicate direction. Resolution: split into `SourceMustAppearInDoc` + `DocMustReferenceSource` variants explicitly.

Plus one not-flagged-as-P0 reviewer note: `NoBareVecConfig` lacks recursion control for nested macros. Resolution: added `recurse_into_nested_macros: bool` (default true) + `max_recursion_depth: USize` (default 8).

Phase 2D implementation can begin against this revised memo.

Phase 2D implementation order, post-memo:

1. Author the eleven reusable primitives + six bespoke primitives in `mockspace-rs/src/builtins/` (one file per primitive).
2. Build the catalog registry with `inventory::collect!` at engine root.
3. Wire the TOML loader (`LintsConfig::load` + `instantiate`) with the `Outcome<_, Vec<ConfigError>>` two-channel return.
4. Build `MockspaceProject` + `MockspaceDocument` concrete types with the cache shapes from §7 and §8.
5. Implement `StagingFilter` (§10).
6. Port the 16 mockspace built-ins to catalog entries; port the 17 stack-lints lints in parallel.
7. Drop 5 safe-duplicate per-repo lints; merge 3; keep 4 as repo-specific catalog entries.
8. Per-repo `mockspace.toml` → `lints.toml` extraction.
9. `cargo mock check` per consumer repo. Catch drift.

Estimated surface: ~5000 lines across mockspace-rs (primitives + builtins module + catalog + project/document types + TOML loader + staging filter). Per-primitive average 300 lines (impl + config + tests + fixtures). Catalog registry under 200 lines. Project/document types under 600 lines. Total estimate within the "code can be naive, viola lands soon" framing.
