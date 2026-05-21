//! Catalog entry registry for the mockspace built-in lint set.
//!
//! Per schema design memo §12. Each lint registers exactly one
//! [`CatalogEntry`] via `inventory::submit!`. The linker concatenates
//! the submitted entries across all crates linked into the host; the
//! engine reads the full set at startup via `inventory::iter::<CatalogEntry>()`.
//!
//! This module covers the 16 mockspace built-in lints (Pool B in the
//! per-lint audit). The 17 stack-lints in
//! `mockspace-hilavitkutin-stack-lints` register their own entries the
//! same way and merge transparently once that crate is depended on.

use mockspace_core::lint::{GateSeverity, Severity};

use crate::catalog::CatalogEntry;
use crate::errors::ConfigError;
use crate::lint::{Lint, LintMode};

use super::ast_type_position;
use super::content_regex;
use super::deprecation_comparison;
use super::directive_style_consistency;
use super::no_adhoc_framework;
use super::no_bare_vec;
use super::no_manual_id;
use super::no_manual_impl;
use super::registrable_completeness;
use super::suppression_meta;
use super::token_scan;

// =========================================================================
// Helper: wrap a primitive's instantiate_with for catalog dispatch.
// =========================================================================
//
// `inventory::submit!` takes a `fn` pointer, not a closure. Each lint
// gets a tiny shim function that threads the lint's name / description /
// default severity into the primitive's `instantiate_with`. Verbose but
// const-friendly and link-time-fixed.

// =========================================================================
// no-alloc: forbid heap allocation types.
// =========================================================================

fn instantiate_no_alloc(
    config: &toml::Table,
    scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    token_scan::instantiate_with(
        "no-alloc",
        "no heap allocation in stack code",
        GateSeverity::uniform(Severity::Error),
        config,
        scope,
    )
}

inventory::submit! {
    CatalogEntry {
        name: "no-alloc",
        description: "no heap allocation in stack code",
        kind: "token-scan",
        default_config: r#"
tokens = ["Vec<", "Box<", "Rc<", "Arc<", "String", "vec!", "HashMap<", "BTreeMap<"]
word_boundary = false
strip_strings = true
strip_comments = true
strip_doc_comments = true
"#,
        default_scope: r#"
paths = ["**/*.rs"]
exempt_paths = ["**/build.rs", "**/benches/**"]
"#,
        default_severity: GateSeverity::uniform(Severity::Error),
        default_impact: None,
        default_category: None,
        doc_url: None,
        mode: LintMode::PerDocument,
        staging_aware: true,
        editor_skip: false,
        instantiate: instantiate_no_alloc,
        finding_kinds: &[],
    }
}

// =========================================================================
// no-std: forbid std imports.
// =========================================================================

fn instantiate_no_std(
    config: &toml::Table,
    scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    token_scan::instantiate_with(
        "no-std",
        "no std imports in stack code",
        GateSeverity::uniform(Severity::Error),
        config,
        scope,
    )
}

inventory::submit! {
    CatalogEntry {
        name: "no-std",
        description: "no std imports in stack code",
        kind: "token-scan",
        default_config: r#"
tokens = ["use std::", "use ::std::", "pub use std::", "pub use ::std::", "extern crate std"]
word_boundary = false
strip_strings = true
strip_comments = true
strip_doc_comments = true
"#,
        default_scope: r#"
paths = ["**/*.rs"]
exempt_paths = ["**/build.rs", "**/benches/**", "**/tests/**"]
"#,
        default_severity: GateSeverity::uniform(Severity::Error),
        default_impact: None,
        default_category: None,
        doc_url: None,
        mode: LintMode::PerDocument,
        staging_aware: true,
        editor_skip: false,
        instantiate: instantiate_no_std,
        finding_kinds: &[],
    }
}

// =========================================================================
// no-dyn-dispatch: forbid runtime polymorphism.
// =========================================================================

fn instantiate_no_dyn_dispatch(
    config: &toml::Table,
    scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    token_scan::instantiate_with(
        "no-dyn-dispatch",
        "no runtime polymorphism via dyn / TypeId",
        GateSeverity::uniform(Severity::Error),
        config,
        scope,
    )
}

inventory::submit! {
    CatalogEntry {
        name: "no-dyn-dispatch",
        description: "no runtime polymorphism via dyn / TypeId",
        kind: "token-scan",
        default_config: r#"
tokens = ["dyn ", "TypeId", "std::any", "core::any", "*const dyn", "*mut dyn", "&dyn ", "&mut dyn "]
word_boundary = false
strip_strings = true
strip_comments = true
strip_doc_comments = true
"#,
        default_scope: r#"
paths = ["**/*.rs"]
"#,
        default_severity: GateSeverity::uniform(Severity::Error),
        default_impact: None,
        default_category: None,
        doc_url: None,
        mode: LintMode::PerDocument,
        staging_aware: true,
        editor_skip: false,
        instantiate: instantiate_no_dyn_dispatch,
        finding_kinds: &[],
    }
}

// =========================================================================
// no-runtime-spawn: forbid thread/task spawn.
// =========================================================================

fn instantiate_no_runtime_spawn(
    config: &toml::Table,
    scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    token_scan::instantiate_with(
        "no-runtime-spawn",
        "no runtime thread/task spawn outside the scheduler",
        GateSeverity::uniform(Severity::Error),
        config,
        scope,
    )
}

inventory::submit! {
    CatalogEntry {
        name: "no-runtime-spawn",
        description: "no runtime thread/task spawn outside the scheduler",
        kind: "token-scan",
        default_config: r#"
tokens = ["thread::spawn", "tokio::spawn", "rayon::spawn", "async_std::spawn", "std::thread::spawn"]
word_boundary = false
strip_strings = true
strip_comments = true
strip_doc_comments = true
"#,
        default_scope: r#"
paths = ["**/*.rs"]
"#,
        default_severity: GateSeverity::uniform(Severity::Error),
        default_impact: None,
        default_category: None,
        doc_url: None,
        mode: LintMode::PerDocument,
        staging_aware: true,
        editor_skip: false,
        instantiate: instantiate_no_runtime_spawn,
        finding_kinds: &[],
    }
}

// =========================================================================
// no-runtime-registration: forbid dynamic plugin patterns.
// =========================================================================

fn instantiate_no_runtime_registration(
    config: &toml::Table,
    scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    token_scan::instantiate_with(
        "no-runtime-registration",
        "no dynamic registration patterns",
        GateSeverity::uniform(Severity::Error),
        config,
        scope,
    )
}

inventory::submit! {
    CatalogEntry {
        name: "no-runtime-registration",
        description: "no dynamic registration patterns",
        kind: "token-scan",
        default_config: r##"
tokens = ["inventory::submit", "linkme::distributed_slice", "#[ctor]", "#[dtor]"]
word_boundary = false
strip_strings = true
strip_comments = true
strip_doc_comments = true
"##,
        default_scope: r#"
paths = ["**/*.rs"]
exempt_crates = ["mockspace-rs", "mockspace-hilavitkutin-stack-lints"]
"#,
        default_severity: GateSeverity::uniform(Severity::Error),
        default_impact: None,
        default_category: None,
        doc_url: None,
        mode: LintMode::PerDocument,
        staging_aware: true,
        editor_skip: false,
        instantiate: instantiate_no_runtime_registration,
        finding_kinds: &[],
    }
}

// =========================================================================
// no-bare-numeric: forbid bare u*/i*/f* in pub API.
// =========================================================================

fn instantiate_no_bare_numeric(
    config: &toml::Table,
    scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    ast_type_position::instantiate_with(
        "no-bare-numeric",
        "no bare numeric primitives in public API",
        GateSeverity::uniform(Severity::Error),
        config,
        scope,
    )
}

inventory::submit! {
    CatalogEntry {
        name: "no-bare-numeric",
        description: "no bare numeric primitives in public API",
        kind: "ast-type-position",
        default_config: r#"
forbidden_types = ["u8", "u16", "u32", "u64", "u128", "i8", "i16", "i32", "i64", "i128", "usize", "isize", "f32", "f64"]
positions = ["fn-param", "fn-return", "struct-field", "enum-variant-field"]
visibility = "public"
"#,
        default_scope: r#"
paths = ["**/*.rs"]
exempt_paths = ["**/ffi/**", "**/build.rs"]
"#,
        default_severity: GateSeverity::uniform(Severity::Error),
        default_impact: None,
        default_category: None,
        doc_url: None,
        mode: LintMode::PerDocument,
        staging_aware: true,
        editor_skip: false,
        instantiate: instantiate_no_bare_numeric,
        finding_kinds: &[],
    }
}

// =========================================================================
// no-bare-string: forbid String / &str in pub API.
// =========================================================================

fn instantiate_no_bare_string(
    config: &toml::Table,
    scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    ast_type_position::instantiate_with(
        "no-bare-string",
        "no bare String / &str in public API",
        GateSeverity::uniform(Severity::Error),
        config,
        scope,
    )
}

inventory::submit! {
    CatalogEntry {
        name: "no-bare-string",
        description: "no bare String / &str in public API",
        kind: "ast-type-position",
        default_config: r#"
forbidden_types = ["String"]
positions = ["fn-param", "fn-return", "struct-field"]
visibility = "public"
exempt_categories = ["string-foundation"]
"#,
        default_scope: r#"
paths = ["**/*.rs"]
"#,
        default_severity: GateSeverity::uniform(Severity::Error),
        default_impact: None,
        default_category: None,
        doc_url: None,
        mode: LintMode::PerDocument,
        staging_aware: true,
        editor_skip: false,
        instantiate: instantiate_no_bare_string,
        finding_kinds: &[],
    }
}

// =========================================================================
// no-bare-option: forbid Option<T> in pub API.
// =========================================================================

fn instantiate_no_bare_option(
    config: &toml::Table,
    scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    ast_type_position::instantiate_with(
        "no-bare-option",
        "no bare Option<T> in public API; use notko::Maybe",
        GateSeverity::uniform(Severity::Warn),
        config,
        scope,
    )
}

inventory::submit! {
    CatalogEntry {
        name: "no-bare-option",
        description: "no bare Option<T> in public API; use notko::Maybe",
        kind: "ast-type-position",
        default_config: r#"
forbidden_types = ["Option"]
positions = ["fn-param", "fn-return", "struct-field"]
visibility = "public"
replacements = [["Option", "Maybe"]]
"#,
        default_scope: r#"
paths = ["**/*.rs"]
exempt_crates = ["notko"]
"#,
        default_severity: GateSeverity::uniform(Severity::Warn),
        default_impact: None,
        default_category: None,
        doc_url: None,
        mode: LintMode::PerDocument,
        staging_aware: true,
        editor_skip: false,
        instantiate: instantiate_no_bare_option,
        finding_kinds: &[],
    }
}

// =========================================================================
// no-bare-result: forbid Result<T, E> in pub API.
// =========================================================================

fn instantiate_no_bare_result(
    config: &toml::Table,
    scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    ast_type_position::instantiate_with(
        "no-bare-result",
        "no bare Result<T, E> in public API; use notko::Outcome / notko::Just",
        GateSeverity::uniform(Severity::Warn),
        config,
        scope,
    )
}

inventory::submit! {
    CatalogEntry {
        name: "no-bare-result",
        description: "no bare Result<T, E> in public API; use notko::Outcome / notko::Just",
        kind: "ast-type-position",
        default_config: r#"
forbidden_types = ["Result"]
positions = ["fn-param", "fn-return"]
visibility = "public"
replacements = [["Result", "Outcome"]]
"#,
        default_scope: r#"
paths = ["**/*.rs"]
exempt_crates = ["notko"]
"#,
        default_severity: GateSeverity::uniform(Severity::Warn),
        default_impact: None,
        default_category: None,
        doc_url: None,
        mode: LintMode::PerDocument,
        staging_aware: true,
        editor_skip: false,
        instantiate: instantiate_no_bare_result,
        finding_kinds: &[],
    }
}

// =========================================================================
// no-public-raw-field: pub struct fields must be typed (no `pub field: u8`).
// =========================================================================

fn instantiate_no_public_raw_field(
    config: &toml::Table,
    scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    ast_type_position::instantiate_with(
        "no-public-raw-field",
        "pub struct fields must use typed newtypes",
        GateSeverity::uniform(Severity::Warn),
        config,
        scope,
    )
}

inventory::submit! {
    CatalogEntry {
        name: "no-public-raw-field",
        description: "pub struct fields must use typed newtypes",
        kind: "ast-type-position",
        default_config: r#"
forbidden_types = ["u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "usize", "isize", "f32", "f64", "bool"]
positions = ["struct-field"]
visibility = "public"
"#,
        default_scope: r#"
paths = ["**/*.rs"]
"#,
        default_severity: GateSeverity::uniform(Severity::Warn),
        default_impact: None,
        default_category: None,
        doc_url: None,
        mode: LintMode::PerDocument,
        staging_aware: true,
        editor_skip: false,
        instantiate: instantiate_no_public_raw_field,
        finding_kinds: &[],
    }
}

// =========================================================================
// no-vec-in-trait-sig: trait method signatures never take/return Vec.
// =========================================================================

fn instantiate_no_vec_in_trait_sig(
    config: &toml::Table,
    scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    ast_type_position::instantiate_with(
        "no-vec-in-trait-sig",
        "trait method signatures must not take or return Vec",
        GateSeverity::uniform(Severity::Error),
        config,
        scope,
    )
}

inventory::submit! {
    CatalogEntry {
        name: "no-vec-in-trait-sig",
        description: "trait method signatures must not take or return Vec",
        kind: "ast-type-position",
        default_config: r#"
forbidden_types = ["Vec"]
positions = ["fn-param", "fn-return"]
visibility = "public"
"#,
        default_scope: r#"
paths = ["**/*.rs"]
"#,
        default_severity: GateSeverity::uniform(Severity::Error),
        default_impact: None,
        default_category: None,
        doc_url: None,
        mode: LintMode::PerDocument,
        staging_aware: true,
        editor_skip: false,
        instantiate: instantiate_no_vec_in_trait_sig,
        finding_kinds: &[],
    }
}

// =========================================================================
// strategy-marker-required: pub numeric types need S: Strategy.
// =========================================================================
//
// This is the placeholder form. The arvo-specific tighter version
// (NUMERIC_CRATES scope + comment-based exemption) stays as a repo-local
// lint per the per-lint audit's "keep 4" group.

fn instantiate_strategy_marker_required(
    config: &toml::Table,
    scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    ast_type_position::instantiate_with(
        "strategy-marker-required",
        "pub numeric types must carry S: Strategy",
        GateSeverity::uniform(Severity::Warn),
        config,
        scope,
    )
}

inventory::submit! {
    CatalogEntry {
        name: "strategy-marker-required",
        description: "pub numeric types must carry S: Strategy",
        kind: "ast-type-position",
        default_config: r#"
forbidden_types = ["UFixed", "IFixed", "Bits"]
positions = ["fn-param", "fn-return", "struct-field"]
visibility = "public"
"#,
        default_scope: r#"
paths = ["**/*.rs"]
crates = ["arvo*"]
"#,
        default_severity: GateSeverity::uniform(Severity::Warn),
        default_impact: None,
        default_category: None,
        doc_url: None,
        mode: LintMode::PerDocument,
        staging_aware: true,
        editor_skip: false,
        instantiate: instantiate_strategy_marker_required,
        finding_kinds: &[],
    }
}

// =========================================================================
// trait-first-signatures: take trait bounds, not concrete collections.
// =========================================================================

fn instantiate_trait_first_signatures(
    config: &toml::Table,
    scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    ast_type_position::instantiate_with(
        "trait-first-signatures",
        "public APIs take trait bounds, not concrete collections",
        GateSeverity::uniform(Severity::Warn),
        config,
        scope,
    )
}

inventory::submit! {
    CatalogEntry {
        name: "trait-first-signatures",
        description: "public APIs take trait bounds, not concrete collections",
        kind: "ast-type-position",
        default_config: r#"
forbidden_types = ["Vec", "HashMap", "BTreeMap", "HashSet", "BTreeSet"]
positions = ["fn-param", "fn-return"]
visibility = "public"
"#,
        default_scope: r#"
paths = ["**/*.rs"]
"#,
        default_severity: GateSeverity::uniform(Severity::Warn),
        default_impact: None,
        default_category: None,
        doc_url: None,
        mode: LintMode::PerDocument,
        staging_aware: true,
        editor_skip: false,
        instantiate: instantiate_trait_first_signatures,
        finding_kinds: &[],
    }
}

// semantic-alias-nudge is intentionally NOT registered here. The lint
// concept ships in mockspace-hilavitkutin-stack-lints with a populated
// arvo-primitive -> alias table; an empty-table registration in this
// crate would be a permanent no-op that masks the real registration
// when the dep is added. The shim lives there.

// =========================================================================
// writing-style: em-dash + marketing word + filler regex pack.
// =========================================================================

fn instantiate_writing_style(
    config: &toml::Table,
    scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    content_regex::instantiate_with(
        "writing-style",
        "writing style: em-dashes, marketing words, filler",
        GateSeverity::uniform(Severity::Warn),
        config,
        scope,
    )
}

inventory::submit! {
    CatalogEntry {
        name: "writing-style",
        description: "writing style: em-dashes, marketing words, filler",
        kind: "content-regex",
        default_config: r#"
[[patterns]]
regex = "—"
message = "em-dashes are forbidden; use period, comma, or parens"
finding_kind = "em-dash"
strip_code_fences = true

[[patterns]]
regex = '\b(leverage|seamless|robust|powerful|holistic|paradigm|unlock|streamline|utilize)\b'
message = "marketing word; rewrite to describe the concrete property"
finding_kind = "marketing-word"
strip_code_fences = true

[[patterns]]
regex = '\b(essentially|basically|fundamentally|literally)\b'
message = "filler word; cut entirely"
finding_kind = "filler"
strip_code_fences = true
"#,
        default_scope: r#"
paths = ["**/*.md", "**/*.md.tmpl"]
"#,
        default_severity: GateSeverity::uniform(Severity::Warn),
        default_impact: None,
        default_category: None,
        doc_url: None,
        mode: LintMode::PerDocument,
        staging_aware: true,
        editor_skip: false,
        instantiate: instantiate_writing_style,
        finding_kinds: &["em-dash", "marketing-word", "filler"],
    }
}

// =========================================================================
// lint-allow-requires-task-id: suppression meta.
// =========================================================================

fn instantiate_lint_allow_requires_task_id(
    config: &toml::Table,
    scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    suppression_meta::instantiate_with(
        "lint-allow-requires-task-id",
        "every lint:allow scope must carry a tracked task id and reason",
        GateSeverity::uniform(Severity::Error),
        config,
        scope,
    )
}

inventory::submit! {
    CatalogEntry {
        name: "lint-allow-requires-task-id",
        description: "every lint:allow scope must carry a tracked task id and reason",
        kind: "suppression-meta",
        default_config: r#"
require_tracked = true
require_reason = true
require_reason_min_words = 3
forbid_expired = false
"#,
        default_scope: r#"
paths = ["**/*.rs"]
"#,
        default_severity: GateSeverity::uniform(Severity::Error),
        default_impact: None,
        default_category: None,
        doc_url: None,
        mode: LintMode::ProjectScoped,
        staging_aware: false,
        editor_skip: true,
        instantiate: instantiate_lint_allow_requires_task_id,
        finding_kinds: &[],
    }
}

// =========================================================================
// directive-style-consistency: enforces uniform comment-form vs attribute-form.
// =========================================================================

fn instantiate_directive_style_consistency(
    config: &toml::Table,
    scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    directive_style_consistency::instantiate_with(
        "directive-style-consistency",
        "enforce project-wide uniformity of directive surface (comment vs attribute)",
        GateSeverity::uniform(Severity::Warn),
        config,
        scope,
    )
}

inventory::submit! {
    CatalogEntry {
        name: "directive-style-consistency",
        description: "enforce project-wide uniformity of directive surface (comment vs attribute)",
        kind: "directive-style-consistency",
        default_config: r#"
style = "mixed"
"#,
        default_scope: r#"
paths = ["**/*.rs"]
"#,
        default_severity: GateSeverity::uniform(Severity::Warn),
        default_impact: None,
        default_category: None,
        doc_url: None,
        mode: LintMode::ProjectScoped,
        staging_aware: false,
        editor_skip: true,
        instantiate: instantiate_directive_style_consistency,
        finding_kinds: &[],
    }
}

// =========================================================================
// Bespoke primitive registrations.
// =========================================================================
//
// Each one re-uses the bespoke's own `instantiate_with` (which takes the
// shipping default config from the catalog string).

fn instantiate_no_bare_vec(
    config: &toml::Table,
    scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    no_bare_vec::instantiate_with(
        "no-bare-vec",
        "no bare Vec / Box / Rc / Arc collection types in stack code",
        GateSeverity::uniform(Severity::Error),
        config,
        scope,
    )
}

inventory::submit! {
    CatalogEntry {
        name: "no-bare-vec",
        description: "no bare Vec / Box / Rc / Arc collection types in stack code",
        kind: "no-bare-vec",
        default_config: r#"
forbidden_types = ["Vec", "Box", "Rc", "Arc", "VecDeque"]
positions = ["fn-param", "fn-return", "struct-field"]
visibility = "public"
macros = []
macro_body_tokens = []
"#,
        default_scope: r#"
paths = ["**/*.rs"]
exempt_paths = ["**/ffi/**"]
"#,
        default_severity: GateSeverity::uniform(Severity::Error),
        default_impact: None,
        default_category: None,
        doc_url: None,
        mode: LintMode::PerDocument,
        staging_aware: true,
        editor_skip: false,
        instantiate: instantiate_no_bare_vec,
        finding_kinds: &[],
    }
}

fn instantiate_no_manual_id(
    config: &toml::Table,
    scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    no_manual_id::instantiate_with(
        "no-manual-id",
        "newtype IDs over primitive integers should use arvo aliases",
        GateSeverity::uniform(Severity::Warn),
        config,
        scope,
    )
}

inventory::submit! {
    CatalogEntry {
        name: "no-manual-id",
        description: "newtype IDs over primitive integers should use arvo aliases",
        kind: "no-manual-id",
        default_config: r#"
primitive_inner_types = ["u8", "u16", "u32", "u64", "i32", "i64", "usize"]
check_aliases = true
"#,
        default_scope: r#"
paths = ["**/*.rs"]
"#,
        default_severity: GateSeverity::uniform(Severity::Warn),
        default_impact: None,
        default_category: None,
        doc_url: None,
        mode: LintMode::PerDocument,
        staging_aware: true,
        editor_skip: false,
        instantiate: instantiate_no_manual_id,
        finding_kinds: &[],
    }
}

fn instantiate_no_manual_impl(
    config: &toml::Table,
    scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    no_manual_impl::instantiate_with(
        "no-manual-impl",
        "manual impls of traits that should be derived",
        GateSeverity::uniform(Severity::Warn),
        config,
        scope,
    )
}

inventory::submit! {
    CatalogEntry {
        name: "no-manual-impl",
        description: "manual impls of traits that should be derived",
        kind: "no-manual-impl",
        default_config: r#"
forbidden_traits = ["Clone", "Copy", "Debug", "Default", "PartialEq", "Eq", "Hash"]
"#,
        default_scope: r#"
paths = ["**/*.rs"]
"#,
        default_severity: GateSeverity::uniform(Severity::Warn),
        default_impact: None,
        default_category: None,
        doc_url: None,
        mode: LintMode::PerDocument,
        staging_aware: true,
        editor_skip: false,
        instantiate: instantiate_no_manual_impl,
        finding_kinds: &[],
    }
}

fn instantiate_no_adhoc_framework(
    config: &toml::Table,
    scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    no_adhoc_framework::instantiate_with(
        "no-adhoc-framework",
        "ad-hoc framework patterns nudge toward hilavitkutin scheduler",
        GateSeverity::uniform(Severity::Hint),
        config,
        scope,
    )
}

inventory::submit! {
    CatalogEntry {
        name: "no-adhoc-framework",
        description: "ad-hoc framework patterns nudge toward hilavitkutin scheduler",
        kind: "no-adhoc-framework",
        default_config: r#"
detect_dispatch_tables = true
detect_lifecycle_triples = true
detect_callback_chains = true
min_signal_count = 3
"#,
        default_scope: r#"
paths = ["**/*.rs"]
"#,
        default_severity: GateSeverity::uniform(Severity::Hint),
        default_impact: None,
        default_category: None,
        doc_url: None,
        mode: LintMode::TwoPhaseProject,
        staging_aware: true,
        editor_skip: true,
        instantiate: instantiate_no_adhoc_framework,
        finding_kinds: &["adhoc-framework"],
    }
}

fn instantiate_registrable_completeness(
    config: &toml::Table,
    scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    registrable_completeness::instantiate_with(
        "registrable-completeness",
        "validates that Registrable trait impls supply required associated items",
        GateSeverity::uniform(Severity::Warn),
        config,
        scope,
    )
}

inventory::submit! {
    CatalogEntry {
        name: "registrable-completeness",
        description: "validates that Registrable trait impls supply required associated items",
        kind: "registrable-completeness",
        default_config: r#"
trait_name = "Registrable"
required_items = []
"#,
        default_scope: r#"
paths = ["**/*.rs"]
"#,
        default_severity: GateSeverity::uniform(Severity::Warn),
        default_impact: None,
        default_category: None,
        doc_url: None,
        mode: LintMode::TwoPhaseProject,
        staging_aware: false,
        editor_skip: true,
        instantiate: instantiate_registrable_completeness,
        finding_kinds: &["incomplete-impl"],
    }
}

fn instantiate_deprecation_comparison(
    config: &toml::Table,
    scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    deprecation_comparison::instantiate_with(
        "deprecation-comparison",
        "drift between deprecated and active changelist symbol sets",
        GateSeverity::uniform(Severity::Hint),
        config,
        scope,
    )
}

inventory::submit! {
    CatalogEntry {
        name: "deprecation-comparison",
        description: "drift between deprecated and active changelist symbol sets",
        kind: "deprecation-comparison",
        default_config: r#"
active_cls_glob = "mock/design_rounds/**/*.lock.md"
deprecated_cls_glob = "mock/design_rounds/**/*.deprecated.md"
symbol_kinds = ["fn", "type", "trait"]
"#,
        default_scope: r#"
paths = ["mock/design_rounds/**/*.md"]
"#,
        default_severity: GateSeverity::uniform(Severity::Hint),
        default_impact: None,
        default_category: None,
        doc_url: None,
        mode: LintMode::TwoPhaseProject,
        staging_aware: false,
        editor_skip: true,
        instantiate: instantiate_deprecation_comparison,
        finding_kinds: &["drift"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::catalog_entries;
    use crate::config_loader::LintsConfig;

    #[test]
    fn registry_contains_expected_entries() {
        let names: Vec<&'static str> = catalog_entries().iter().map(|e| e.name).collect();
        // 16 mockspace built-ins registered above (subset shown).
        for expected in &[
            "no-alloc",
            "no-std",
            "no-dyn-dispatch",
            "no-runtime-spawn",
            "no-bare-numeric",
            "no-bare-string",
            "no-bare-option",
            "no-bare-result",
            "no-public-raw-field",
            "no-vec-in-trait-sig",
            "strategy-marker-required",
            "trait-first-signatures",
            "writing-style",
            "lint-allow-requires-task-id",
            "no-runtime-registration",
            "no-bare-vec",
            "no-manual-id",
            "no-manual-impl",
            "no-adhoc-framework",
            "registrable-completeness",
            "deprecation-comparison",
        ] {
            assert!(
                names.contains(expected),
                "catalog missing entry: {expected}"
            );
        }
    }

    #[test]
    fn catalog_defaults_instantiate_without_config_errors() {
        let cfg = LintsConfig::from_catalog_defaults();
        assert!(
            cfg.config_errors.is_empty(),
            "config errors: {:?}",
            cfg.config_errors
        );
        assert_eq!(cfg.entries.len(), catalog_entries().len());
    }

    #[test]
    fn every_catalog_entry_default_config_parses_and_instantiates() {
        // Exercise each entry's default_config + default_scope strings
        // via its own instantiate fn. Catches TOML drift or schema
        // typos that would otherwise only surface at first engine load.
        for entry in catalog_entries() {
            let config: toml::Table = entry.default_config.parse().unwrap_or_else(|e| {
                panic!(
                    "catalog entry `{}` default_config TOML parse failed: {e}",
                    entry.name
                )
            });
            let scope: toml::Table = entry.default_scope.parse().unwrap_or_else(|e| {
                panic!(
                    "catalog entry `{}` default_scope TOML parse failed: {e}",
                    entry.name
                )
            });
            (entry.instantiate)(&config, &scope).unwrap_or_else(|e| {
                panic!("catalog entry `{}` instantiate failed: {e}", entry.name)
            });
        }
    }

    #[test]
    fn no_duplicate_catalog_names() {
        let mut names: Vec<&'static str> = catalog_entries().iter().map(|e| e.name).collect();
        names.sort();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "duplicate names: {names:?}");
    }
}
