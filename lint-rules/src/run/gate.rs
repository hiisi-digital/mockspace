//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Whether a lint runs at all, and the severity its findings carry once it has.

use crate::{Lint, LintConfig, LintError};

/// Whether `lint` runs in this run.
///
/// A configured override wins; absent one, the lint's own declared default decides. Naming a lint in
/// any section of the configuration counts as asking for it: a consumer can configure only
/// `findings` or only `params`, and reading that as silence would make the configuration inert
/// without saying so. A source-only lint does not run in a doc-only run.
pub(crate) fn enabled(lint: &dyn Lint, doc_only: bool, overrides: Option<&LintConfig>) -> bool {
    if doc_only && lint.source_only() {
        return false;
    }
    let name = lint.name();
    let named_in_config = overrides.is_some_and(|cfg| {
        cfg.base.contains_key(name)
            || cfg.findings.contains_key(name)
            || cfg.params.contains_key(name)
            || cfg.paths.contains_key(name)
            || cfg.crates.contains_key(name)
    });
    if !named_in_config && lint.default_severity().is_off() {
        return false;
    }
    !overrides.is_some_and(|cfg| cfg.base.get(name).is_some_and(|sev| sev.is_off()))
}

/// Put the configured severity on each finding, where one is configured.
///
/// Only an explicit override restamps. A lint's own per-finding levels are deliberate gradation, a
/// build gate and an advisory coming out of one lint, and flattening them onto the lint's default
/// would erase it. Applied to replayed findings exactly as to fresh ones, which is why a severity
/// is no part of what a saved unit is keyed on.
pub(crate) fn restamp(lint: &dyn Lint, overrides: Option<&LintConfig>, findings: &mut [LintError]) {
    let Some(cfg) = overrides else { return };
    let base = cfg.base.get(lint.name()).copied();
    let kinds = cfg.findings.get(lint.name());
    for err in findings {
        let effective = match (err.finding_kind, kinds) {
            (Some(kind), Some(map)) => map.get(kind).copied().or(base),
            _ => base,
        };
        if let Some(sev) = effective {
            err.severity = sev;
        }
    }
}

/// Hand each of a freshly built built-in set its configured parameters.
pub(crate) fn configure_all<'a>(lints: impl IntoIterator<Item = &'a mut dyn Lint>, overrides: Option<&LintConfig>) {
    let Some(cfg) = overrides else { return };
    for lint in lints {
        cfg.apply_params_to(lint);
    }
}
