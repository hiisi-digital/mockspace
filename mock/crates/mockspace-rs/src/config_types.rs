//! Shared per-primitive configuration enums.
//!
//! Vocabulary used across multiple primitive `Config` types: visibility
//! filters, type-position enums, item-kind enums, language tags, etc.
//! Per-primitive bespoke fields live in each primitive's own module.

// Re-export language tag from mockspace-core so primitives can name it
// without dragging in the core path everywhere.
pub use mockspace_core::lint::Language;
use serde::{Deserialize, Serialize};

/// Type-bearing AST position. Used by `AstTypePosition` and `NoBareVec`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TypePosition {
    /// `struct Foo { field: T }`.
    StructField,
    /// `enum Foo { Variant(T) }` or `Variant { field: T }`.
    EnumVariantField,
    /// `fn foo(param: T)`.
    FnParam,
    /// `fn foo() -> T`.
    FnReturn,
    /// `type Foo = T;`.
    TypeAliasBody,
    /// `trait Foo { type Bar; }` / `impl Foo { type Bar = T; }`.
    AssociatedType,
}

/// Item kind discriminator used by `IdentifierPattern`, `UndocumentedItem`,
/// and `RegistrableCompleteness`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
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

/// Visibility filter for items / positions. Per-primitive, never per-scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    /// Match any item regardless of visibility.
    #[default]
    Any,
    /// Match only `pub`-visible items / positions.
    Public,
}

/// A unified scope block applied per-lint at engine load.
///
/// Per schema design memo §5. Path/crate globs, language filter, category
/// exemptions. No `visibility` field; visibility lives on each primitive's
/// `Config`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct ScopeConfig {
    /// File-system globs (globset syntax) for files this lint sees.
    #[serde(default)]
    pub paths: Vec<String>,

    /// File-system globs to exempt.
    #[serde(default)]
    pub exempt_paths: Vec<String>,

    /// Crate name patterns. `*` matches all crates.
    #[serde(default)]
    pub crates: Vec<String>,

    /// Crate name patterns to exempt.
    #[serde(default)]
    pub exempt_crates: Vec<String>,

    /// Language filter (Rust, Markdown, ...).
    #[serde(default)]
    pub languages: Vec<Language>,

    /// Exempt all crates listed in workspace `proc_macro_crates`.
    #[serde(default)]
    pub proc_macro_exempt: bool,
}

/// Per-gate configuration block.
///
/// Per schema §6. Per-finding-kind severity overrides validated against
/// the lint's `finding_kinds` at instantiate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GateConfig {
    pub severity: mockspace_core::lint::Severity,

    #[serde(default)]
    pub only_staged: bool,

    #[serde(default)]
    pub skip: bool,

    /// Optional per-finding-kind severity overrides. Keys must appear in
    /// the lint's `CatalogEntry::finding_kinds`.
    #[serde(default)]
    pub finding_kinds: Option<std::collections::HashMap<String, mockspace_core::lint::Severity>>,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            severity:      mockspace_core::lint::Severity::Off,
            only_staged:   false,
            skip:          false,
            finding_kinds: None,
        }
    }
}

/// A simple match-count escalation rule reused across primitives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EscalationRule {
    pub threshold:          u32,
    pub escalated_severity: mockspace_core::lint::Severity,
}
