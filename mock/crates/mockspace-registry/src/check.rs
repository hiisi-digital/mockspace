//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a registry is held to, as one call.
//!
//! Each check is a function of its own in this crate, and which of them run, in what order and what
//! an unverifiable shape counts as is decided here and nowhere else, so two tools that keep a
//! registry agree on what a passing one is by calling this and not by listing the checks alike.
//! Printing a finding, counting it against a run and scoping it to the files a run changed are the
//! caller's: a finding says which runs it blocks (`gate`) and the caller knows which run it is.

use crate::{GateScope, Registry, RegistryEnv, RegistryFinding, SchemaCheck, VerdictCache};

/// What [`check`] found, in the order it is reported.
pub struct Check {
    /// What the rows and the declarations are held to. Each blocks the runs its `gate` names.
    pub findings:   Vec<RegistryFinding>,
    /// Keys of the registry's tables of the config that nothing reads, so the declaration they sit
    /// in does less than it says. Advisory unless the caller's project says otherwise: an inert key
    /// does not stop the registry from being correct.
    pub inert_keys: Vec<RegistryFinding>,
    /// Whether the rows have the shape their namespaces declare.
    pub shape:      Shape,
}

/// The verdict of the generated schemas over the rows.
pub enum Shape {
    /// Every row has the shape its namespace declares.
    Passed,
    /// There is no row, so there was nothing to verify and no validator was needed.
    NothingToVerify,
    /// The rows that do not, or the one finding that says the shape of every row went unverified
    /// because no validator is installed. Each blocks every run.
    Refused(Vec<RegistryFinding>),
}

impl Check {
    /// Every finding that is not advisory: the rows', the declarations' and the shape's.
    pub fn blocking(&self) -> impl Iterator<Item = &RegistryFinding> {
        let shape: &[RegistryFinding] = match &self.shape {
            Shape::Refused(found) => found,
            Shape::Passed | Shape::NothingToVerify => &[],
        };
        self.findings.iter().chain(shape)
    }
}

/// Holds `registry` to everything a registry is held to.
///
/// `cycles` is what resolving the rows reported, which only the caller that resolved them has.
/// `config_text` is the text of the project's config, for the keys nothing reads; a caller that
/// could not read the file passes none and says so itself. `saved` is where the shape's verdict is
/// replayed from, for a caller that keeps verdicts. The schemas are read as they are on disk, so
/// the caller generates them first ([`crate::generate_schemas`]).
pub fn check(
    env: &dyn RegistryEnv,
    registry: &Registry,
    cycles: &[RegistryFinding],
    config_text: Option<&str>,
    saved: Option<&dyn VerdictCache>,
) -> Check {
    let namespaces = env.namespaces();
    let mut findings = cycles.to_vec();
    findings.extend(crate::validate_provenance(env.repo_root(), env.roots(), env.frozen_roots(), registry, namespaces));
    findings.extend(crate::namespace_root_collisions(namespaces, env.roots()));
    findings.extend(crate::validate(namespaces, registry));
    // The declarations, then the data. There is no gate between them: `row_reference_fields`
    // already skips a field whose type names no namespace, so an unknown type suppresses its own
    // field's rows and nothing else. A project-wide gate stood here and suppressed genuine findings
    // in unrelated namespaces, which cost an author a second round of failures after fixing a typo
    // somewhere they were not looking.
    findings.extend(crate::unknown_field_types(namespaces));
    findings.extend(crate::namespace_type_collisions(namespaces));
    findings.extend(crate::value_field_targets(namespaces));
    findings.extend(crate::misplaced_bounds(namespaces));
    findings.extend(crate::unfit_values(namespaces));
    findings.extend(crate::validate_row_references(registry, namespaces));

    let inert_keys = config_text.map(crate::config_unknown_keys).unwrap_or_default();

    let shape = match crate::check_schemas(env.repo_root(), namespaces, saved) {
        SchemaCheck::Ran {
            failures,
        } if failures.is_empty() => Shape::Passed,
        SchemaCheck::Ran {
            failures,
        } => Shape::Refused(
            failures
                .into_iter()
                .map(|message| RegistryFinding {
                    kind: "schema",
                    message,
                    source: None,
                    gate: GateScope::Always,
                })
                .collect(),
        ),
        // A run that could not check is not a run that passed. An unverifiable row shape reported
        // as success is a green that means nothing: every required field, every type and every
        // slug pattern went unchecked.
        //
        // Decided on rows existing, not on namespaces existing. Two namespaces are built in, so the
        // namespaces are never empty and a test of them fails every project that has no registry
        // at all. Rows are the thing a schema verifies. No rows, nothing unverified.
        SchemaCheck::Unavailable if registry.rows.is_empty() => Shape::NothingToVerify,
        SchemaCheck::Unavailable => Shape::Refused(vec![RegistryFinding {
            kind:    "schema-unavailable",
            message: format!(
                "taplo is not installed, so the shape of {} row(s) is unverified: required fields, types and slug \
                 patterns were all unchecked. Install taplo.",
                registry.rows.len()
            ),
            source:  None,
            gate:    GateScope::Always,
        }]),
    };
    Check {
        findings,
        inert_keys,
        shape,
    }
}
