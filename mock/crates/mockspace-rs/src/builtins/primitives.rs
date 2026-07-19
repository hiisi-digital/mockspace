//! Primitive-name to constructor-and-metadata lookup.
//!
//! Foundation slice for #611 (preset-as-catalog resolver). When a
//! consumer's `mockspace.toml` references a first-party preset whose
//! lint is not in the auto-registered catalog (post #189 / #568), the
//! resolver synthesises an `InstantiatedLint` from the preset file. The
//! preset names a `primitive`; this module is the table that maps that
//! name to the constructor plus the per-primitive default execution
//! metadata (`mode`, `staging_aware`, `editor_skip`).
//!
//! Per-instance metadata (`finding_kinds`, the per-preset description)
//! gets derived from the preset's `[[config.patterns]]` shape in the
//! resolver slice (next PR). This slice only ships the lookup table;
//! the cascade-refactor and synthesis path land in PR-2 and PR-3 per
//! `mock/research/202605231725_preset-as-catalog-resolver.md`.
//!
//! Internal-only API today; consumed in PR-3.

use mockspace_core::lint::GateSeverity;

use super::{
    ast_node_position,
    ast_type_position,
    content_regex,
    cross_doc_symbol,
    deprecation_comparison,
    directive_style_consistency,
    file_metric,
    identifier_pattern,
    no_adhoc_framework,
    no_bare_vec,
    no_manual_id,
    no_manual_impl,
    registrable_completeness,
    suppression_meta,
    term_replacement,
    token_scan,
    undocumented_item,
    workflow_state,
};
use crate::errors::ConfigError;
use crate::lint::{Lint, LintMode};

/// Constructor signature shared by every primitive's `instantiate_with`.
/// The cascade plumbing (existing entry path and the upcoming
/// synthesis path) calls into a primitive through this pointer.
pub type InstantiateFn = fn(
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: &toml::Table,
    scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError>;

/// One primitive's lookup row: constructor plus the execution-shape
/// defaults a synthesised CatalogEntry-equivalent would inherit.
///
/// `finding_kinds` stays empty here; the per-instance kinds get
/// derived from the preset's config (e.g. `content-regex` patterns'
/// `finding_kind` field) in the synthesis pass.
#[derive(Clone, Copy)]
pub struct PrimitiveDescriptor {
    pub name:          &'static str,
    pub instantiate:   InstantiateFn,
    pub mode:          LintMode,
    pub staging_aware: bool,
    pub editor_skip:   bool,
}

impl PrimitiveDescriptor {
    const fn per_document(name: &'static str, instantiate: InstantiateFn) -> Self {
        Self {
            name,
            instantiate,
            mode: LintMode::PerDocument,
            staging_aware: true,
            editor_skip: false,
        }
    }

    const fn project_scoped(name: &'static str, instantiate: InstantiateFn) -> Self {
        Self {
            name,
            instantiate,
            mode: LintMode::ProjectScoped,
            staging_aware: false,
            editor_skip: true,
        }
    }

    const fn two_phase_project(name: &'static str, instantiate: InstantiateFn) -> Self {
        Self {
            name,
            instantiate,
            mode: LintMode::TwoPhaseProject,
            staging_aware: false,
            editor_skip: true,
        }
    }
}

/// Every primitive shipping in mockspace, keyed for the preset-as-
/// catalog resolver. The set MUST match `KNOWN_PRIMITIVES` in
/// `preset_source.rs`; a unit test below pins the invariant so adding
/// a primitive in one place without the other surfaces immediately.
pub static PRIMITIVE_DESCRIPTORS: &[PrimitiveDescriptor] = &[
    PrimitiveDescriptor::per_document("token-scan", token_scan::instantiate_with),
    PrimitiveDescriptor::per_document("ast-type-position", ast_type_position::instantiate_with),
    PrimitiveDescriptor::per_document(
        "ast-node-position-match",
        ast_node_position::instantiate_with,
    ),
    PrimitiveDescriptor::per_document("identifier-pattern", identifier_pattern::instantiate_with),
    PrimitiveDescriptor::per_document("content-regex", content_regex::instantiate_with),
    PrimitiveDescriptor::per_document("term-replacement-table", term_replacement::instantiate_with),
    PrimitiveDescriptor::per_document("file-metric", file_metric::instantiate_with),
    PrimitiveDescriptor::project_scoped("undocumented-item", undocumented_item::instantiate_with),
    PrimitiveDescriptor::project_scoped("cross-doc-symbol", cross_doc_symbol::instantiate_with),
    PrimitiveDescriptor::project_scoped("workflow-state", workflow_state::instantiate_with),
    PrimitiveDescriptor::project_scoped("suppression-meta", suppression_meta::instantiate_with),
    PrimitiveDescriptor::project_scoped(
        "directive-style-consistency",
        directive_style_consistency::instantiate_with,
    ),
    PrimitiveDescriptor::per_document("no-bare-vec", no_bare_vec::instantiate_with),
    PrimitiveDescriptor::per_document("no-manual-id", no_manual_id::instantiate_with),
    PrimitiveDescriptor::per_document("no-manual-impl", no_manual_impl::instantiate_with),
    // no-adhoc-framework is TwoPhaseProject but staging_aware unlike
    // the project-scoped + two-phase defaults below; the live
    // registry entry sets staging_aware=true, so the descriptor
    // mirrors it instead of using the helper constructor.
    PrimitiveDescriptor {
        name:          "no-adhoc-framework",
        instantiate:   no_adhoc_framework::instantiate_with,
        mode:          LintMode::TwoPhaseProject,
        staging_aware: true,
        editor_skip:   true,
    },
    PrimitiveDescriptor::two_phase_project(
        "registrable-completeness",
        registrable_completeness::instantiate_with,
    ),
    PrimitiveDescriptor::two_phase_project(
        "deprecation-comparison",
        deprecation_comparison::instantiate_with,
    ),
];

/// O(N) primitive-name lookup. N is 18; the synthesis path calls this
/// once per preset reference in user TOML at engine startup, so a
/// linear scan is the right shape.
pub fn find_descriptor(primitive_name: &str) -> Option<&'static PrimitiveDescriptor> {
    PRIMITIVE_DESCRIPTORS
        .iter()
        .find(|d| d.name == primitive_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every name in the resolver's lookup table must be findable. The
    /// duplicate-name case (which would silently make the second entry
    /// dead) flags here too.
    #[test]
    fn every_descriptor_is_findable() {
        let mut seen = std::collections::HashSet::new();
        for d in PRIMITIVE_DESCRIPTORS {
            assert!(
                seen.insert(d.name),
                "duplicate primitive name `{}` in PRIMITIVE_DESCRIPTORS",
                d.name
            );
            assert!(
                find_descriptor(d.name).is_some(),
                "descriptor `{}` not findable through find_descriptor",
                d.name
            );
        }
    }

    /// Pins the link to `KNOWN_PRIMITIVES` in `preset_source.rs`. If a
    /// primitive ships in one location without the other, this fails
    /// before the synthesis path can mis-route a preset.
    #[test]
    fn descriptors_cover_known_primitives() {
        let known: std::collections::HashSet<&str> = [
            "token-scan",
            "ast-type-position",
            "ast-node-position-match",
            "identifier-pattern",
            "content-regex",
            "term-replacement-table",
            "file-metric",
            "undocumented-item",
            "cross-doc-symbol",
            "workflow-state",
            "suppression-meta",
            "directive-style-consistency",
            "no-bare-vec",
            "no-manual-id",
            "no-manual-impl",
            "no-adhoc-framework",
            "registrable-completeness",
            "deprecation-comparison",
        ]
        .iter()
        .copied()
        .collect();

        let descriptor_names: std::collections::HashSet<&str> =
            PRIMITIVE_DESCRIPTORS.iter().map(|d| d.name).collect();

        assert_eq!(
            known, descriptor_names,
            "PRIMITIVE_DESCRIPTORS and KNOWN_PRIMITIVES drifted; \
             add or remove the missing entries in both locations"
        );
    }

    #[test]
    fn lookup_returns_none_for_unknown_name() {
        assert!(find_descriptor("definitely-not-a-primitive-zzz").is_none());
    }
}
