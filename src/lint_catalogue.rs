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
    /// The base severity this project configured, where it configured one.
    ///
    /// `None` is not a fault and is the interesting case: the lint runs at its
    /// default in a project that never gave it a base severity. It is a
    /// narrower question than [`Listing::named`], and reading it as the wider
    /// one is what this field used to be used for.
    pub confirmed: Option<Severity>,
    /// Whether the project named this lint in its configuration at all.
    ///
    /// A `[lints.<name>]` table carrying only `findings`, only `params` or only
    /// path filters populates no base severity and is still a decision the
    /// project made. The engine counts it as one, in
    /// `configure_and_run`'s `named_in_config`, and a catalogue that counted
    /// only `base` reported four such shapes as "pack default" while the engine
    /// treated them as configured. One of those directions is dangerous rather
    /// than merely wrong: a lint whose pack default is `OFF`, named only under
    /// `findings`, runs, and the catalogue said `OFF`.
    pub named:     bool,
}

impl Listing {
    /// The base severity that governs: the configured one, else the default.
    ///
    /// Per-finding severities are not folded in, because there is no single
    /// answer to fold: `findings` maps a finding kind to a severity, and a lint
    /// with several kinds has several. [`Listing::runs`] is the question that
    /// has one answer.
    #[must_use]
    pub fn effective(&self) -> Severity {
        self.confirmed.unwrap_or(self.default)
    }

    /// Whether this project made a decision about this lint.
    #[must_use]
    pub fn is_configured(&self) -> bool {
        self.named
    }

    /// Whether the engine will actually run it, by the engine's own rule.
    ///
    /// Mirrors `configure_and_run`: a lint is skipped when its default is off
    /// and nothing named it, and skipped when a base override turns it off.
    /// Anything else runs, including a lint whose default is off that the
    /// project named without giving it a base severity.
    #[must_use]
    pub fn runs(&self) -> bool {
        if self.confirmed.is_some_and(|s| s.is_off()) {
            return false;
        }
        self.named || !self.default.is_off()
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

/// Whether the project named this lint anywhere in its configuration.
///
/// The same four sections `configure_and_run` reads, in the same order. Spelled
/// once here so a fifth section added to [`LintConfig`] is one edit rather than
/// two places that agree until they do not.
fn named_in_config(name: &str, config: &LintConfig) -> bool {
    config.base.contains_key(name)
        || config.findings.contains_key(name)
        || config.params.contains_key(name)
        || config.paths.contains_key(name)
}

fn one(name: &str, kind: Kind, default: Severity, config: &LintConfig) -> Listing {
    Listing {
        name: name.to_string(),
        kind,
        default,
        confirmed: config.base.get(name).copied(),
        named: named_in_config(name, config),
    }
}

/// Lint names the pack registers more than once, sorted, deduplicated.
///
/// Two lints sharing a name list twice while one `[lints.<name>]` table governs
/// both, so the configuration a reader writes against the listing does not do
/// what the listing implies. The tool catalogue reports this for tools; lints
/// come from the same packs and collide the same way.
///
/// Unlike a duplicate tool name it is not refused, because two lints with one
/// name both run and both report, which is untidy rather than ambiguous. A tool
/// has to pick one to execute and cannot.
#[must_use]
pub fn duplicates(pack: &LintPack) -> Vec<String> {
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for name in pack
        .crate_lints
        .iter()
        .map(|l| l.name())
        .chain(pack.workspace_lints.iter().map(|l| l.name()))
        .chain(pack.repo_lints.iter().map(|l| l.name()))
        .chain(pack.message_lints.iter().map(|l| l.name()))
    {
        *seen.entry(name).or_default() += 1;
    }
    seen.into_iter()
        .filter(|&(_, n)| n > 1)
        .map(|(name, _)| name.to_string())
        .collect()
}

/// Configuration written for a name no lint in the pack answers to.
///
/// Sorted. Empty is the ordinary case. A non-empty answer is a table governing
/// nothing, which reads exactly like one that governs something.
///
/// **Every section counts, not only `base`.** A `[lints.<gone>.findings]` table
/// or a bare `[lints.<gone>] include = [...]` is exactly the thing this reports,
/// a table reading as a live decision and governing nothing, and reading only
/// `base` missed the whole of that half.
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
        .chain(config.findings.keys())
        .chain(config.params.keys())
        .chain(config.paths.keys())
        .filter(|k| !present.contains_key(k.as_str()))
        .cloned()
        .collect();
    out.sort();
    out.dedup();
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
        let one = absent.len() == 1;
        let _ = writeln!(
            s,
            "\n{} configured name{} match{} no lint in the pack, so {} govern{} nothing:",
            absent.len(),
            if one { "" } else { "s" },
            if one { "es" } else { "" },
            if one { "it" } else { "they" },
            if one { "s" } else { "" }
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
        let source = if l.is_configured() { "set here" } else { "pack default" };
        // A lint the project named without giving it a base severity runs, and
        // its base severity is the pack's, which can be off. Printing the
        // severity alone reads as "this is not running" in exactly that case,
        // which is the one where it is.
        let note = if l.runs() { "" } else { "  (not run)" };
        let _ = writeln!(
            out,
            "    {:<width$}  {:<9?}  {}{}",
            l.name,
            l.effective(),
            source,
            note,
            width = width
        );
    }
}

#[cfg(test)]
mod tests {
    use mockspace_lint_rules::{
        CrateLint,
        Lint,
        LintContext,
        LintError,
        PathFilter,
        WorkspaceLint,
    };

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

    // A second kind, so a name colliding ACROSS kinds can be planted. The
    // duplicate check reads all four lists, and a fixture that can only build
    // one of them would let a check reading a single list pass.
    impl WorkspaceLint for Named {
        fn check_all(&self, _crates: &[(&str, &LintContext)]) -> Vec<LintError> {
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
        let config =
            config_with(&[("real", Severity::HARD_ERROR), ("renamed-away", Severity::HARD_ERROR)]);
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

    /// A project may name a lint without giving it a base severity.
    ///
    /// **Four shapes, and the catalogue used to read none of them.** The engine
    /// counts any of the four sections as naming the lint, so a catalogue
    /// reading only `base` disagreed with the thing it exists to describe.
    #[test]
    fn naming_a_lint_in_any_section_counts_as_configuring_it() {
        let pack = pack_with(vec![Box::new(Named("named", Severity::ADVISORY))]);

        let mut findings_only = LintConfig::empty();
        findings_only.findings.insert(
            "named".to_string(),
            [("a-kind".to_string(), Severity::HARD_ERROR)]
                .into_iter()
                .collect(),
        );

        let mut params_only = LintConfig::empty();
        params_only.params.insert(
            "named".to_string(),
            [("k".to_string(), "v".to_string())].into_iter().collect(),
        );

        let mut paths_only = LintConfig::empty();
        paths_only.paths.insert("named".to_string(), PathFilter {
            include: vec!["src/**".to_string()],
            exclude: Vec::new(),
        });

        for (what, config) in
            [("findings", findings_only), ("params", params_only), ("paths", paths_only)]
        {
            let listings = enumerate(&pack, &config);
            assert!(
                listings[0].is_configured(),
                "a lint named only under `{what}` is one the project decided about, and the \
                 engine treats it as such"
            );
            assert!(
                listings[0].confirmed.is_none(),
                "and it still has no base severity, which is the narrower question `{what}` does \
                 not answer"
            );
        }
    }

    /// The dangerous direction, and the reason the two questions had to split.
    ///
    /// A pack default of `OFF` plus a name under `findings` is a lint the engine
    /// runs. The catalogue printed `OFF` and called it a pack default, so a
    /// reader checking what was checking them was told the opposite.
    #[test]
    fn a_lint_off_by_default_and_named_only_under_findings_reports_as_running() {
        let pack = pack_with(vec![Box::new(Named("woken", Severity::OFF))]);
        let mut config = LintConfig::empty();
        config.findings.insert(
            "woken".to_string(),
            [("a-kind".to_string(), Severity::HARD_ERROR)]
                .into_iter()
                .collect(),
        );
        let listings = enumerate(&pack, &config);
        assert!(
            listings[0].runs(),
            "the engine skips a lint only when nothing named it and its default is off; this one \
             was named"
        );
        let table = render_table(&listings, &[]);
        assert!(
            !table.contains("(not run)"),
            "and the table must not say otherwise: {table}"
        );
    }

    /// The negative control for the above, or `runs` could be a constant.
    #[test]
    fn a_lint_off_by_default_that_nobody_named_reports_as_not_running() {
        let pack = pack_with(vec![Box::new(Named("asleep", Severity::OFF))]);
        let listings = enumerate(&pack, &LintConfig::empty());
        assert!(
            !listings[0].runs(),
            "nothing named it and its default is off"
        );
        let table = render_table(&listings, &[]);
        assert!(table.contains("(not run)"), "{table}");
    }

    /// And a base severity of `OFF` turns one off whatever else named it.
    #[test]
    fn an_explicit_off_stops_a_lint_that_would_otherwise_run() {
        let pack = pack_with(vec![Box::new(Named("silenced", Severity::HARD_ERROR))]);
        let config = config_with(&[("silenced", Severity::OFF)]);
        let listings = enumerate(&pack, &config);
        assert!(!listings[0].runs());
    }

    /// The cross-check's other half, which never fired.
    ///
    /// A stale `[lints.<gone>.findings]` table is precisely "a table that reads
    /// as a live decision and governs nothing", and reading only `base` meant
    /// the one section most likely to be left behind by a rename was the one
    /// section never checked.
    #[test]
    fn a_stale_table_carrying_no_base_severity_is_still_reported_as_absent() {
        let pack = pack_with(vec![Box::new(Named("real", Severity::ADVISORY))]);
        let mut config = LintConfig::empty();
        config.findings.insert(
            "renamed-away".to_string(),
            [("a-kind".to_string(), Severity::HARD_ERROR)]
                .into_iter()
                .collect(),
        );
        config.params.insert(
            "also-gone".to_string(),
            [("k".to_string(), "v".to_string())].into_iter().collect(),
        );
        assert_eq!(configured_but_absent(&pack, &config), vec![
            "also-gone".to_string(),
            "renamed-away".to_string()
        ],);
    }

    /// One name in several sections is one finding, not three.
    #[test]
    fn a_stale_name_in_several_sections_is_reported_once() {
        let pack = pack_with(vec![Box::new(Named("real", Severity::ADVISORY))]);
        let mut config = config_with(&[("gone", Severity::HARD_ERROR)]);
        config.params.insert(
            "gone".to_string(),
            [("k".to_string(), "v".to_string())].into_iter().collect(),
        );
        assert_eq!(configured_but_absent(&pack, &config), vec![
            "gone".to_string()
        ]);
    }

    /// The absent branch renders, which nothing checked and it shipped wrong.
    ///
    /// Both call sites passed an empty slice, so the singular and plural arms
    /// had never run. The first real invocation printed "1 configured severity
    /// name no lint in the pack, so it govern nothing".
    #[test]
    fn the_absent_line_agrees_with_itself_in_both_numbers() {
        let listings = enumerate(&LintPack::default(), &LintConfig::empty());

        let one = render_table(&listings, &["gone".to_string()]);
        assert!(
            one.contains("1 configured name matches no lint in the pack, so it governs nothing:"),
            "the singular arm has to inflect every verb in it, not one of them: {one}"
        );
        assert!(one.contains("    gone"), "and name the offender: {one}");

        let many = render_table(&listings, &["gone".to_string(), "also-gone".to_string()]);
        assert!(
            many.contains("2 configured names match no lint in the pack, so they govern nothing:"),
            "{many}"
        );
    }

    /// Two lints under one name, which one `[lints.<name>]` table governs.
    #[test]
    fn a_name_registered_twice_is_reported_as_a_duplicate() {
        let pack = pack_with(vec![
            Box::new(Named("twice", Severity::ADVISORY)),
            Box::new(Named("twice", Severity::HARD_ERROR)),
            Box::new(Named("once", Severity::ADVISORY)),
        ]);
        assert_eq!(duplicates(&pack), vec!["twice".to_string()]);
    }

    /// The control, so it is not reporting every name it sees.
    #[test]
    fn a_pack_with_distinct_names_reports_no_duplicates() {
        let pack = pack_with(vec![
            Box::new(Named("a", Severity::ADVISORY)),
            Box::new(Named("b", Severity::ADVISORY)),
        ]);
        assert!(duplicates(&pack).is_empty());
    }

    /// A name colliding across two kinds is still one name.
    #[test]
    fn a_name_registered_in_two_different_kinds_is_a_duplicate() {
        let pack = LintPack {
            crate_lints: vec![Box::new(Named("shared", Severity::ADVISORY))],
            workspace_lints: vec![Box::new(Named("shared", Severity::ADVISORY))],
            ..Default::default()
        };
        assert_eq!(duplicates(&pack), vec!["shared".to_string()]);
    }
}
