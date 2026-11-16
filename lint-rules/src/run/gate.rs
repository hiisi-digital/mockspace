//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Whether a lint runs at all, and the severity its findings carry once it has.

use crate::{Level, Lint, LintConfig, LintError, Severity};

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
///
/// A lint may keep the gates a finding deliberately passes at ([`Lint::override_keeps_passes`]),
/// so a lint-wide `commit = "error"` does not raise them; a severity named for the finding kind
/// itself always sets all three gates.
pub(crate) fn restamp(lint: &dyn Lint, overrides: Option<&LintConfig>, findings: &mut [LintError]) {
    let Some(cfg) = overrides else { return };
    let base = cfg.base.get(lint.name()).copied();
    let kinds = cfg.findings.get(lint.name());
    for err in findings {
        let named = err.finding_kind.zip(kinds).and_then(|(kind, map)| map.get(kind).copied());
        if let Some(sev) = named {
            err.severity = sev;
        } else if let Some(sev) = base {
            err.severity = match err.finding_kind {
                // The lint-wide severity reaches only the gates the finding acts at, where the
                // lint says its passes are deliberate.
                Some(kind) if lint.override_keeps_passes(kind) => keep_passes(err.severity, sev),
                _ => sev,
            };
        }
    }
}

/// `over` at each gate where `own` is not `Pass`, and `Pass` where it is.
fn keep_passes(own: Severity, over: Severity) -> Severity {
    let at = |own: Level, over: Level| if own == Level::Pass { Level::Pass } else { over };
    Severity::new(
        at(own.on_commit, over.on_commit),
        at(own.on_build, over.on_build),
        at(own.on_push, over.on_push),
    )
}

/// Hand each of a freshly built built-in set its configured parameters.
pub(crate) fn configure_all<'a>(lints: impl IntoIterator<Item = &'a mut dyn Lint>, overrides: Option<&LintConfig>) {
    let Some(cfg) = overrides else { return };
    for lint in lints {
        cfg.apply_params_to(lint);
    }
}
