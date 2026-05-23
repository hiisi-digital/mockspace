//! Catalog entry registry for the mockspace built-in bespoke lint set.
//!
//! Per schema design memo §12. Each lint registers exactly one
//! [`CatalogEntry`] via `inventory::submit!`. The linker concatenates
//! the submitted entries across all crates linked into the host; the
//! engine reads the full set at startup via `inventory::iter::<CatalogEntry>()`.
//!
//! This module covers the 7 mockspace built-in bespoke lints whose
//! identity is the impl rather than a configurable primitive:
//! `directive-style-consistency`, `no-bare-vec`, `no-manual-id`,
//! `no-manual-impl`, `no-adhoc-framework`, `registrable-completeness`,
//! `deprecation-comparison`. The 15 preset-replaceable lints
//! (`no-alloc`, `no-std`, `no-dyn-dispatch`, `no-runtime-spawn`,
//! `no-runtime-registration`, `no-bare-numeric`, `no-bare-string`,
//! `no-bare-option`, `no-bare-result`, `no-public-raw-field`,
//! `no-vec-in-trait-sig`, `strategy-marker-required`,
//! `trait-first-signatures`, `writing-style`,
//! `lint-allow-requires-task-id`) live as preset files under
//! `mockspace-rs/presets/<name>.toml` and load via the preset resolver
//! when a consumer's `mockspace.toml` references them with
//! `extends = "mockspace::<name>"`. They are NOT auto-registered; a
//! consumer who wants any of them MUST opt in explicitly.
//!
//! See `docs/MIGRATION-v1-to-v2-lints.md` for the consumer migration
//! steps when picking up this change.
//!
//! Stack-lints in `mockspace-hilavitkutin-stack-lints` register their
//! own entries the same `inventory::submit!` way as the bespoke lints
//! here and merge transparently once that crate is depended on.

use mockspace_core::lint::{GateSeverity, Severity};

use crate::catalog::CatalogEntry;
use crate::errors::ConfigError;
use crate::lint::{Lint, LintMode};

use super::deprecation_comparison;
use super::directive_style_consistency;
use super::no_adhoc_framework;
use super::no_bare_vec;
use super::no_manual_id;
use super::no_manual_impl;
use super::registrable_completeness;

// =========================================================================
// Helper: wrap a primitive's instantiate_with for catalog dispatch.
// =========================================================================
//
// `inventory::submit!` takes a `fn` pointer, not a closure. Each lint
// gets a tiny shim function that threads the lint's name / description /
// default severity into the primitive's `instantiate_with`. Verbose but
// const-friendly and link-time-fixed.

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
    fn registry_contains_expected_bespoke_entries() {
        let names: Vec<&'static str> = catalog_entries().iter().map(|e| e.name).collect();
        // Only the 7 bespoke lints are auto-registered. The 15
        // preset-replaceable lints ship as `presets/<name>.toml` files
        // and load via the preset resolver when a consumer's
        // `mockspace.toml` explicitly references them with
        // `extends = "mockspace::<name>"`. See #568.
        for expected in &[
            "directive-style-consistency",
            "no-bare-vec",
            "no-manual-id",
            "no-manual-impl",
            "no-adhoc-framework",
            "registrable-completeness",
            "deprecation-comparison",
        ] {
            assert!(
                names.contains(expected),
                "catalog missing bespoke entry: {expected}"
            );
        }
    }

    #[test]
    fn registry_does_not_auto_register_preset_replaced_lints() {
        let names: Vec<&'static str> = catalog_entries().iter().map(|e| e.name).collect();
        // The 15 preset-replaceable lints MUST NOT appear in the
        // auto-registered catalog. They are opt-in via
        // `extends = "mockspace::<name>"` only. See #568.
        for absent in &[
            "no-alloc",
            "no-std",
            "no-dyn-dispatch",
            "no-runtime-spawn",
            "no-runtime-registration",
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
        ] {
            assert!(
                !names.contains(absent),
                "preset-replaced lint should not auto-register: {absent}"
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
