//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! One enumeration of every lint, with the severity this project gave it.
//!
//! # Why this exists
//!
//! [`crate::tool_catalogue`] answers "what can I run here" for tools, and
//! nothing answered "what is checking me" for lints. The set was recoverable
//! only by grepping the pack's source for a naming convention not every file
//! follows, which under-counts silently: two extraction attempts against one
//! pack returned one lint and then twenty-four, against fifty-nine severities
//! the same project declared in its `mockspace.toml`.
//!
//! That is the asymmetry this closes. A lint already declares its own name and
//! its own default severity, exactly as a tool declares its name and summary,
//! so the authoritative set was always in the pack and only ever needed asking.
//!
//! # The cross-check is the point, not the list
//!
//! A list of lints is mildly useful. What is worth a subcommand is that the
//! two sides can disagree and nothing reports it:
//!
//! - **A severity configured for a lint that does not exist.** A rename in the
//!   pack, or a lint dropped from it, leaves a `[lints.<name>]` table behind
//!   that reads as a live decision and governs nothing. Silent, because
//!   configuration for an absent lint is not an error anywhere.
//! - **A lint with no configured severity.** It runs at whatever default the
//!   pack chose, in a project that never named it. That is the case where the
//!   pack author decided for the consumer, and a consumer who has never
//!   written the name down cannot have disagreed.
//!
//! Neither is presented as a fault, because neither always is: a project may
//! legitimately take a pack's default, and a stale table may be a rename in
//! flight. They are reported so the decision is visible rather than implied.
//!
//! # Scope
//!
//! **Every lint the pack carries, whatever kind.** The four kinds differ in
//! what the engine hands them and not in whether they check you, so a listing
//! that showed one kind would be the same under-count in a different shape.

use std::collections::BTreeMap;

use mockspace_lint_rules::{LintConfig, LintPack, Severity};

/// Which input kind a lint is handed.
///
/// Kept as a label rather than folded away, because the kind is what decides
/// when a lint runs, and a reader asking what checks a crate wants a different
/// answer than one asking what checks a commit message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// Handed one crate at a time.
    Crate,
    /// Handed every crate at once.
    Workspace,
    /// Handed repository state, with no crates.
    Repo,
    /// Handed an authored message.
    Message,
}

impl Kind {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Crate => "CRATE LINTS",
            Self::Workspace => "WORKSPACE LINTS",
            Self::Repo => "REPO LINTS",
            Self::Message => "MESSAGE LINTS",
        }
    }
}

/// One lint, as the catalogue reports it.
#[derive(Debug, Clone)]
pub struct Listing {
    /// The name the lint answers to, and the key a `[lints.<name>]` table uses.
    pub name:      String,
    /// What kind of input it is handed.
    pub kind:      Kind,
    /// The severity the lint itself declares when the project says nothing.
    pub default:   Severity,
    /// The severity this project configured, where it configured one.
    ///
    /// `None` is not a fault and is the interesting case: the lint runs at its
    /// default in a project that never named it.
    pub confirmed: Option<Severity>,
}

impl Listing {
    /// The severity that actually governs: the configured one, else the default.
    #[must_use]
    pub fn effective(&self) -> Severity {
        self.confirmed.unwrap_or(self.default)
    }

    /// Whether this project made a decision about this lint.
    #[must_use]
    pub fn is_configured(&self) -> bool {
        self.confirmed.is_some()
    }
}

/// Every lint in the pack, with the severity this project gave it.
///
/// Sorted by kind and then by name, because the pack's own order is a
/// declaration order nobody reading a listing cares about, and a stable order
/// is what makes two runs comparable.
#[must_use]
pub fn enumerate(pack: &LintPack, config: &LintConfig) -> Vec<Listing> {
    let mut out: Vec<Listing> = Vec::new();

    for l in &pack.crate_lints {
        out.push(one(l.name(), Kind::Crate, l.default_severity(), config));
    }
    for l in &pack.workspace_lints {
        out.push(one(l.name(), Kind::Workspace, l.default_severity(), config));
    }
    for l in &pack.repo_lints {
        out.push(one(l.name(), Kind::Repo, l.default_severity(), config));
    }
    for l in &pack.message_lints {
        out.push(one(l.name(), Kind::Message, l.default_severity(), config));
    }

    out.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
    out
}

fn one(name: &str, kind: Kind, default: Severity, config: &LintConfig) -> Listing {
    Listing {
        name: name.to_string(),
        kind,
        default,
        confirmed: config.base.get(name).copied(),
    }
}

/// Severities configured for a name no lint in the pack answers to.
///
/// Sorted. Empty is the ordinary case. A non-empty answer is a table governing
/// nothing, which reads exactly like one that governs something.
#[must_use]
pub fn configured_but_absent(pack: &LintPack, config: &LintConfig) -> Vec<String> {
    let present: BTreeMap<&str, ()> = pack
        .crate_lints
        .iter()
        .map(|l| l.name())
        .chain(pack.workspace_lints.iter().map(|l| l.name()))
        .chain(pack.repo_lints.iter().map(|l| l.name()))
        .chain(pack.message_lints.iter().map(|l| l.name()))
        .map(|n| (n, ()))
        .collect();

    let mut out: Vec<String> = config
        .base
        .keys()
        .filter(|k| !present.contains_key(k.as_str()))
        .cloned()
        .collect();
    out.sort();
    out
}

/// The table `mock lints` prints.
#[must_use]
pub fn render_table(listings: &[Listing], absent: &[String]) -> String {
    let mut s = String::new();
    for kind in [Kind::Crate, Kind::Workspace, Kind::Repo, Kind::Message] {
        render_group(&mut s, kind, listings);
        s.push('\n');
    }

    let unconfigured = listings.iter().filter(|l| !l.is_configured()).count();
    use std::fmt::Write as _;
    let _ = writeln!(
        s,
        "{} lints, {} configured here, {} running at the pack's default.",
        listings.len(),
        listings.len() - unconfigured,
        unconfigured
    );

    if !absent.is_empty() {
        let _ = writeln!(
            s,
            "\n{} configured severit{} name no lint in the pack, so {} govern nothing:",
            absent.len(),
            if absent.len() == 1 { "y" } else { "ies" },
            if absent.len() == 1 { "it" } else { "they" }
        );
        for name in absent {
            let _ = writeln!(s, "    {name}");
        }
    }

    s
}

fn render_group(out: &mut String, kind: Kind, listings: &[Listing]) {
    let group: Vec<&Listing> = listings.iter().filter(|l| l.kind == kind).collect();
    out.push_str(kind.label());
    out.push('\n');
    if group.is_empty() {
        out.push_str("    (none)\n");
        return;
    }
    let width = group.iter().map(|l| l.name.len()).max().unwrap_or(0);
    for l in group {
        use std::fmt::Write as _;
        let source = if l.is_configured() {
            "set here"
        } else {
            "pack default"
        };
        let _ = writeln!(
            out,
            "    {:<width$}  {:<9?}  {}",
            l.name,
            l.effective(),
            source,
            width = width
        );
    }
}

#[cfg(test)]
mod tests {
    use mockspace_lint_rules::{CrateLint, Lint, LintContext, LintError};

    use super::*;

    struct Named(&'static str, Severity);

    impl Lint for Named {
        fn name(&self) -> &'static str {
            self.0
        }

        fn default_severity(&self) -> Severity {
            self.1
        }
    }

    impl CrateLint for Named {
        fn check(&self, _ctx: &LintContext) -> Vec<LintError> {
            Vec::new()
        }
    }

    fn pack_with(lints: Vec<Box<dyn CrateLint>>) -> LintPack {
        LintPack {
            crate_lints: lints,
            ..Default::default()
        }
    }

    fn config_with(pairs: &[(&str, Severity)]) -> LintConfig {
        let mut c = LintConfig::empty();
        for (k, v) in pairs {
            c.base.insert((*k).to_string(), *v);
        }
        c
    }

    /// The control. Without it every assertion below is satisfied by an
    /// enumeration that returns nothing at all.
    #[test]
    fn a_lint_in_the_pack_is_listed() {
        let pack = pack_with(vec![Box::new(Named("a-lint", Severity::ADVISORY))]);
        let listings = enumerate(&pack, &LintConfig::empty());
        assert_eq!(listings.len(), 1);
        assert_eq!(listings[0].name, "a-lint");
        assert_eq!(listings[0].kind, Kind::Crate);
    }

    /// The half nothing reported before: a lint nobody configured runs anyway.
    #[test]
    fn a_lint_the_project_never_named_reports_as_running_at_the_default() {
        let pack = pack_with(vec![Box::new(Named("unchosen", Severity::ADVISORY))]);
        let listings = enumerate(&pack, &LintConfig::empty());
        assert!(
            !listings[0].is_configured(),
            "a lint with no configured severity must report as unconfigured"
        );
        assert_eq!(
            listings[0].effective(),
            Severity::ADVISORY,
            "and must report the default as what actually governs"
        );
    }

    /// A configured severity wins, and the listing says which side it came from.
    #[test]
    fn a_configured_severity_overrides_the_default_and_is_marked_as_chosen() {
        let pack = pack_with(vec![Box::new(Named("chosen", Severity::ADVISORY))]);
        let config = config_with(&[("chosen", Severity::HARD_ERROR)]);
        let listings = enumerate(&pack, &config);
        assert!(listings[0].is_configured());
        assert_eq!(listings[0].effective(), Severity::HARD_ERROR);
    }

    /// The other half: a table that governs nothing, which reads like one that does.
    #[test]
    fn a_severity_naming_no_lint_is_reported_as_governing_nothing() {
        let pack = pack_with(vec![Box::new(Named("real", Severity::ADVISORY))]);
        let config = config_with(&[
            ("real", Severity::HARD_ERROR),
            ("renamed-away", Severity::HARD_ERROR),
        ]);
        assert_eq!(
            configured_but_absent(&pack, &config),
            vec!["renamed-away".to_string()],
            "a severity keyed to no lint must be named, and one keyed to a real lint must not"
        );
    }

    /// The negative half of the one above, so it is not passing by returning everything.
    #[test]
    fn a_pack_whose_every_lint_is_configured_reports_nothing_absent() {
        let pack = pack_with(vec![Box::new(Named("real", Severity::ADVISORY))]);
        let config = config_with(&[("real", Severity::HARD_ERROR)]);
        assert!(configured_but_absent(&pack, &config).is_empty());
    }

    /// An empty pack is a legitimate state and must not be a panic or a lie.
    #[test]
    fn an_empty_pack_enumerates_to_nothing_rather_than_failing() {
        let listings = enumerate(&LintPack::default(), &LintConfig::empty());
        assert!(listings.is_empty());
        let table = render_table(&listings, &[]);
        assert!(
            table.contains("0 lints"),
            "an empty pack must say so rather than rendering an empty table: {table}"
        );
    }

    /// The rendered table has to carry the distinction, not only the struct.
    #[test]
    fn the_table_says_which_side_each_severity_came_from() {
        let pack = pack_with(vec![
            Box::new(Named("chosen", Severity::ADVISORY)),
            Box::new(Named("unchosen", Severity::ADVISORY)),
        ]);
        let config = config_with(&[("chosen", Severity::HARD_ERROR)]);
        let table = render_table(&enumerate(&pack, &config), &[]);
        assert!(table.contains("set here"), "{table}");
        assert!(table.contains("pack default"), "{table}");
    }
}
