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
//! so the authoritative set was always there to be asked for rather than
//! grepped. Asked of two places: the engine's builtins and the loaded pack.
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
//! **Every lint that runs here, whatever kind and wherever it comes from.** The
//! engine's builtins and the loaded pack both, because both run at every gate,
//! and a listing of one of them is the under-count this exists to close. The
//! four kinds differ in
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
    /// `run_with_overrides`'s `named_in_config`, and a catalogue that counted
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

    /// Whether the engine runs it at an ordinary gate.
    ///
    /// Two of `run_with_overrides`'s three skip conditions: a lint is skipped
    /// when its default is off and nothing named it, and skipped when a base
    /// override turns it off.
    ///
    /// **The third is not modelled and cannot be, from here.** A `--doc-only`
    /// run also skips every lint whose `source_only` is true, which defaults to
    /// true, so in that mode nearly everything is skipped. This takes no such
    /// flag, so it describes the ordinary gate and says so rather than claiming
    /// to mirror the engine in every mode.
    #[must_use]
    pub fn runs(&self) -> bool {
        if self.confirmed.is_some_and(|s| s.is_off()) {
            return false;
        }
        self.named || !self.default.is_off()
    }
}

/// Every lint that runs here, with the severity this project gave it.
///
/// **The builtins and the pack, because the engine runs both.** A `LintPack`
/// holds what this repository and its declared lint crates contribute, and the
/// engine adds `all_lints`, `all_workspace_lints` and `all_repo_lints` at every
/// gate. Reading only the pack was the shape this module inherited from
/// [`crate::tool_catalogue`], where it is correct: a tool exists only in the
/// pack. A lint does not, and the difference is the whole of what went wrong.
///
/// What it cost, before the fix: the command whose entire purpose is answering
/// what is checking you omitted 29 lints that were, and the cross-check below
/// then reported every configured builtin as a name governing nothing. Against
/// one real project that was 15 of its 59 configured names, each of them
/// governing a lint that runs.
///
/// Sorted by kind and then by name, because declaration order is not something
/// a reader of a listing cares about, and a stable order is what makes two runs
/// comparable.
#[must_use]
pub fn enumerate(pack: &LintPack, config: &LintConfig) -> Vec<Listing> {
    let mut out: Vec<Listing> = Vec::new();

    for l in mockspace_lint_rules::all_lints() {
        out.push(one(l.name(), Kind::Crate, l.default_severity(), config));
    }
    for l in mockspace_lint_rules::all_workspace_lints() {
        out.push(one(l.name(), Kind::Workspace, l.default_severity(), config));
    }
    for l in mockspace_lint_rules::all_repo_lints() {
        out.push(one(l.name(), Kind::Repo, l.default_severity(), config));
    }

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
/// The same four sections `run_with_overrides` reads, in the same order. Spelled
/// once here so a fifth section added to [`LintConfig`] is one edit rather than
/// two places that agree until they do not.
fn named_in_config(name: &str, config: &LintConfig) -> bool {
    config.base.contains_key(name)
        || config.findings.contains_key(name)
        || config.params.contains_key(name)
        || config.paths.contains_key(name)
        || config.crates.contains_key(name)
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

/// Lint names registered more than once, sorted, deduplicated.
///
/// The engine's own [`mockspace_lint_rules::duplicate_lint_names`], rather than
/// a second implementation beside it. This module briefly had one that counted
/// the pack against itself, which cannot see the collision that actually
/// happens: a pack shadowing a builtin. That one is fatal, because
/// `run_lints` returns early on it and every gate then refuses, and `mock lints`
/// is the command somebody would run to find out why.
///
/// Kept as a named function here so the subcommand reads against this module
/// like the rest of the listing does.
#[must_use]
pub fn duplicates(pack: &LintPack) -> Vec<String> {
    mockspace_lint_rules::duplicate_lint_names(pack)
}

/// Configuration written for a name no lint here answers to.
///
/// Sorted. Empty is the ordinary case. A non-empty answer is a table governing
/// nothing, which reads exactly like one that governs something.
///
/// **Every section counts, not only `base`.** A `[lints.<gone>.findings]` table
/// or a bare `[lints.<gone>] include = [...]` is exactly the thing this reports,
/// a table reading as a live decision and governing nothing, and reading only
/// `base` missed the whole of that half.
///
/// **And the builtins count as present.** Asking only the pack made this report
/// every configured builtin as governing nothing, which is the opposite of true
/// and is worse than reporting nothing at all: a reader acting on it would
/// delete the configuration for a lint that runs.
#[must_use]
pub fn configured_but_absent(pack: &LintPack, config: &LintConfig) -> Vec<String> {
    let present: BTreeMap<String, ()> = enumerate(pack, config)
        .into_iter()
        .map(|l| (l.name, ()))
        .collect();

    let mut out: Vec<String> = config
        .base
        .keys()
        .chain(config.findings.keys())
        .chain(config.params.keys())
        .chain(config.paths.keys())
        .chain(config.crates.keys())
        .filter(|k| !present.contains_key(*k))
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
            "\n{} configured name{} match{} no lint that runs here, so {} govern{} nothing:",
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
        // `label()` rather than the derived `Debug`. `Severity` is three fields,
        // so `{:?}` prints `Severity { on_commit: Warn, on_build: Warn, ... }`,
        // 58 characters where a word belongs, and a width specifier does not
        // apply to it, so every following column stops lining up. This output
        // is embedded verbatim into the generated agent rules.
        let _ = writeln!(
            out,
            "    {:<width$}  {:<10}  {}{}",
            l.name,
            l.effective().label(),
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
        MessageContext,
        MessageLint,
        PathFilter,
        RepoContext,
        RepoLint,
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

    // All four kinds, so an arm that stops enumerating one of them has
    // something to fail. Three of the four had nothing.
    impl RepoLint for Named {
        fn check_repo(&self, _ctx: &RepoContext) -> Vec<LintError> {
            Vec::new()
        }
    }

    impl MessageLint for Named {
        fn check_message(&self, _ctx: &MessageContext) -> Vec<LintError> {
            Vec::new()
        }
    }

    fn pack_with(lints: Vec<Box<dyn CrateLint>>) -> LintPack {
        LintPack {
            crate_lints: lints,
            ..Default::default()
        }
    }

    /// The listing for one lint by name.
    ///
    /// Every arm below used to index position zero, which held only while the
    /// listing was the pack and the pack was one lint. It is the builtins plus
    /// the pack now, so position says nothing and the name is the only handle
    /// that means anything.
    fn listing<'a>(listings: &'a [Listing], name: &str) -> &'a Listing {
        listings
            .iter()
            .find(|l| l.name == name)
            .unwrap_or_else(|| panic!("`{name}` is not in the listing"))
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
        assert_eq!(listing(&listings, "a-lint").kind, Kind::Crate);
    }

    /// The half nothing reported before: a lint nobody configured runs anyway.
    #[test]
    fn a_lint_the_project_never_named_reports_as_running_at_the_default() {
        let pack = pack_with(vec![Box::new(Named("unchosen", Severity::ADVISORY))]);
        let listings = enumerate(&pack, &LintConfig::empty());
        let l = listing(&listings, "unchosen");
        assert!(
            !l.is_configured(),
            "a lint with no configured severity must report as unconfigured"
        );
        assert_eq!(
            l.effective(),
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
        let l = listing(&listings, "chosen");
        assert!(l.is_configured());
        assert_eq!(l.effective(), Severity::HARD_ERROR);
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
    ///
    /// It no longer enumerates to nothing, and that is the fix rather than a
    /// regression: a repository with no pack still has every builtin checking
    /// it. What is asserted is that the render holds up, not that the answer is
    /// empty.
    #[test]
    fn an_empty_pack_renders_rather_than_failing() {
        let listings = enumerate(&LintPack::default(), &LintConfig::empty());
        let table = render_table(&listings, &[]);
        assert!(
            table.contains(&format!("{} lints", listings.len())),
            "the count has to match what was listed: {table}"
        );
        assert!(
            !listings.is_empty(),
            "and a repository with no pack is still being checked by the builtins"
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
            let l = listing(&listings, "named");
            assert!(
                l.is_configured(),
                "a lint named only under `{what}` is one the project decided about, and the \
                 engine treats it as such"
            );
            assert!(
                l.confirmed.is_none(),
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
            listing(&listings, "woken").runs(),
            "the engine skips a lint only when nothing named it and its default is off; this one \
             was named"
        );
        let row = render_table(&listings, &[])
            .lines()
            .find(|l| l.contains("woken"))
            .expect("listed")
            .to_string();
        assert!(
            !row.contains("(not run)"),
            "and its row must not say otherwise: {row}"
        );
    }

    /// The negative control for the above, or `runs` could be a constant.
    #[test]
    fn a_lint_off_by_default_that_nobody_named_reports_as_not_running() {
        let pack = pack_with(vec![Box::new(Named("asleep", Severity::OFF))]);
        let listings = enumerate(&pack, &LintConfig::empty());
        assert!(
            !listing(&listings, "asleep").runs(),
            "nothing named it and its default is off"
        );
        let row = render_table(&listings, &[])
            .lines()
            .find(|l| l.contains("asleep"))
            .expect("listed")
            .to_string();
        assert!(row.contains("(not run)"), "{row}");
    }

    /// And a base severity of `OFF` turns one off whatever else named it.
    #[test]
    fn an_explicit_off_stops_a_lint_that_would_otherwise_run() {
        let pack = pack_with(vec![Box::new(Named("silenced", Severity::HARD_ERROR))]);
        let config = config_with(&[("silenced", Severity::OFF)]);
        let listings = enumerate(&pack, &config);
        assert!(!listing(&listings, "silenced").runs());
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
    /// name no lint that runs here, so it govern nothing".
    #[test]
    fn the_absent_line_agrees_with_itself_in_both_numbers() {
        let listings = enumerate(&LintPack::default(), &LintConfig::empty());

        let one = render_table(&listings, &["gone".to_string()]);
        assert!(
            one.contains(
                "1 configured name matches no lint that runs here, so it governs nothing:"
            ),
            "the singular arm has to inflect every verb in it, not one of them: {one}"
        );
        assert!(one.contains("    gone"), "and name the offender: {one}");

        let many = render_table(&listings, &["gone".to_string(), "also-gone".to_string()]);
        assert!(
            many.contains(
                "2 configured names match no lint that runs here, so they govern nothing:"
            ),
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

    /// The builtins are listed, and an empty pack is not an empty answer.
    ///
    /// **The defect this module shipped with.** Every function here read the
    /// pack and only the pack, which is right for a tool and wrong for a lint:
    /// the engine adds `all_lints`, `all_workspace_lints` and `all_repo_lints`
    /// at every gate and none of them is ever a pack member. So the command
    /// answering what is checking you could not see most of it.
    #[test]
    fn the_builtins_the_engine_runs_are_listed_even_with_an_empty_pack() {
        let listings = enumerate(&LintPack::default(), &LintConfig::empty());
        let expected = mockspace_lint_rules::all_lints().len()
            + mockspace_lint_rules::all_workspace_lints().len()
            + mockspace_lint_rules::all_repo_lints().len();
        assert!(
            expected > 0,
            "the engine ships builtins, or this arm proves nothing"
        );
        assert_eq!(
            listings.len(),
            expected,
            "an empty pack still has every builtin checking you"
        );
    }

    /// Every kind the pack can carry is enumerated, one arm each.
    ///
    /// **Three of the four arms were unguarded and deleting any of them left
    /// the suite green.** Not hypothetical: the shared pack this workspace uses
    /// registers one workspace lint and three message lints, so the kinds
    /// nothing pinned are kinds a consumer actually has. Under-counting is the
    /// one failure this module exists to prevent, and it had the same defect in
    /// the pack direction that it had just been fixed for in the builtin one.
    #[test]
    fn a_pack_lint_of_every_kind_is_enumerated_under_its_own_kind() {
        let pack = LintPack {
            crate_lints: vec![Box::new(Named("a-crate-lint", Severity::ADVISORY))],
            workspace_lints: vec![Box::new(Named("a-workspace-lint", Severity::ADVISORY))],
            repo_lints: vec![Box::new(Named("a-repo-lint", Severity::ADVISORY))],
            message_lints: vec![Box::new(Named("a-message-lint", Severity::ADVISORY))],
            ..Default::default()
        };
        let listings = enumerate(&pack, &LintConfig::empty());
        for (name, kind) in [
            ("a-crate-lint", Kind::Crate),
            ("a-workspace-lint", Kind::Workspace),
            ("a-repo-lint", Kind::Repo),
            ("a-message-lint", Kind::Message),
        ] {
            assert_eq!(
                listing(&listings, name).kind,
                kind,
                "`{name}` has to be listed, under its own kind: deleting the arm that lists it \
                 must not leave this suite green"
            );
        }
    }

    /// And a pack lint joins them rather than replacing them.
    #[test]
    fn a_pack_lint_joins_the_builtins_rather_than_replacing_them() {
        let builtins = enumerate(&LintPack::default(), &LintConfig::empty()).len();
        let pack = pack_with(vec![Box::new(Named("from-a-pack", Severity::ADVISORY))]);
        let listings = enumerate(&pack, &LintConfig::empty());
        assert_eq!(listings.len(), builtins + 1);
        assert!(listings.iter().any(|l| l.name == "from-a-pack"));
    }

    /// A configured builtin is not a name governing nothing.
    ///
    /// The cross-check's false-positive half, and the dangerous direction:
    /// acting on the old output meant deleting the configuration for a lint
    /// that runs.
    #[test]
    fn a_configured_builtin_is_not_reported_as_governing_nothing() {
        let builtins = mockspace_lint_rules::all_lints();
        let name = builtins
            .first()
            .expect("the engine ships at least one crate lint")
            .name();
        let config = config_with(&[(name, Severity::HARD_ERROR)]);
        assert!(
            !configured_but_absent(&LintPack::default(), &config).contains(&name.to_string()),
            "`{name}` is a lint the engine runs, so configuring it governs something"
        );
    }

    /// A pack shadowing a builtin is the collision that actually happens.
    ///
    /// It is also the fatal one: the engine returns early on it and every lint
    /// gate then refuses. A check counting the pack against itself reported
    /// clean here, which is the state somebody would run `mock lints` to
    /// diagnose.
    #[test]
    fn a_pack_lint_shadowing_a_builtin_is_a_duplicate() {
        let builtins = mockspace_lint_rules::all_lints();
        let name: &'static str = builtins
            .first()
            .expect("the engine ships at least one crate lint")
            .name();
        let pack = pack_with(vec![Box::new(Named(name, Severity::ADVISORY))]);
        assert!(
            duplicates(&pack).contains(&name.to_string()),
            "a pack registering `{name}` shadows the builtin of that name, and nothing else \
             reports it"
        );
    }

    /// The severity column is a word rather than a struct dump.
    ///
    /// `Severity` is three fields with a derived `Debug`, which a width
    /// specifier does not apply to, so `{:?}` printed 58 characters and took
    /// every column after it out of alignment. This output is embedded verbatim
    /// into the generated agent rules.
    #[test]
    fn the_rendered_row_names_the_severity_rather_than_dumping_it() {
        let pack = pack_with(vec![Box::new(Named("a-lint", Severity::ADVISORY))]);
        let table = render_table(&enumerate(&pack, &LintConfig::empty()), &[]);
        let row = table
            .lines()
            .find(|l| l.contains("a-lint"))
            .expect("the lint is listed");
        assert!(
            !row.contains("on_commit"),
            "the derived Debug leaked into the table: {row}"
        );
        assert!(
            row.contains(Severity::ADVISORY.label()),
            "the row has to name the severity that governs: {row}"
        );
    }
}
