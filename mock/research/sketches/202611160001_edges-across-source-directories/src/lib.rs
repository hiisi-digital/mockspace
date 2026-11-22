//! The lint this sketch asks about: a dependency edge between two crates is allowed or refused by the
//! pair of source directories the two crates sit in, as `src_dirs` in `mockspace.toml` spells them.
//!
//! One judgement, two ways of gathering the edges it judges:
//!
//! - [`ByManifest`], a repo lint. It takes the declared source directories from the engine
//!   (`RepoContext::src_dirs`) and reads every package manifest under them through the view.
//! - [`ByEngineDeps`], a workspace lint. It takes each crate's directory and the dependency names
//!   the engine's discovery already computed (`LintContext::crate_dir`, `LintContext::deps`).
//!
//! The pairs are configured as named rules, the shape `forbidden-imports` already reads:
//!
//! ```toml
//! [lints.an-edge-goes-where-its-directories-allow.rule.sim-takes-base]
//! from = "sim"
//! to = "base"
//! ```
//!
//! An edge inside one directory is allowed. An edge across two is allowed only where a rule names
//! that pair, in that direction. Everything else is refused.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use mockspace_lint_rules::src_layout::SrcLayout;
use mockspace_lint_rules::{Lint, LintContext, LintError, RepoContext, RepoLint, Severity, View, WorkspaceLint};

/// The repo-lint arm's name, and the key its rules are configured under.
pub const NAME: &str = "an-edge-goes-where-its-directories-allow";
/// The workspace-lint arm's name.
pub const NAME_BY_ENGINE: &str = "an-edge-goes-where-its-directories-allow-by-engine-deps";

/// The dependency tables of a manifest that make an edge.
const SECTIONS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// One configured rule: an edge from `from` onto `to` is allowed.
#[derive(Clone, Debug)]
pub struct Rule {
    pub name: String,
    pub from: String,
    pub to:   String,
}

/// The allowed pairs, read from a lint's parameters.
#[derive(Clone, Debug, Default)]
pub struct Pairs {
    pub rules:     Vec<Rule>,
    /// Rules missing `from` or `to`, by rule name.
    pub malformed: Vec<String>,
}

impl Pairs {
    /// The rules in `rule.<name>.from` and `rule.<name>.to`, which is how the engine flattens
    /// `[lints.<lint>.rule.<name>]`.
    #[must_use]
    pub fn from_params(params: &HashMap<String, String>) -> Self {
        let mut by_name: BTreeMap<String, (Option<String>, Option<String>)> = BTreeMap::new();
        for (key, value) in params {
            let Some(rest) = key.strip_prefix("rule.") else { continue };
            let Some((name, field)) = rest.rsplit_once('.') else { continue };
            let slot = by_name.entry(name.to_owned()).or_default();
            match field {
                "from" => slot.0 = Some(value.trim().trim_end_matches('/').to_owned()),
                "to" => slot.1 = Some(value.trim().trim_end_matches('/').to_owned()),
                _ => {},
            }
        }
        let mut out = Pairs::default();
        for (name, slot) in by_name {
            match slot {
                (Some(from), Some(to)) if !from.is_empty() && !to.is_empty() => out.rules.push(Rule { name, from, to }),
                _ => out.malformed.push(name),
            }
        }
        out
    }

    /// The rule that allows `from -> to`, if one does.
    #[must_use]
    pub fn rule_for(&self, from: &str, to: &str) -> Option<&Rule> {
        self.rules.iter().find(|r| r.from == from && r.to == to)
    }
}

/// One dependency edge, with the source directory of each end.
#[derive(Clone, Debug)]
pub struct Edge {
    pub from_crate: String,
    pub from_dir:   String,
    pub to_crate:   String,
    pub to_dir:     String,
    /// The manifest table it was read from, or what the engine's deps say of it.
    pub section:    String,
    /// The manifest naming it, relative to the repository root, where the arm read one.
    pub manifest:   Option<String>,
    pub line:       usize,
}

/// What the lint says of one edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Both ends in one directory.
    Within,
    /// Across directories, allowed by the named rule.
    Allowed(String),
    /// Across directories, and no rule names the pair.
    Refused,
}

/// The one judgement both arms share.
#[must_use]
pub fn judge(pairs: &Pairs, edge: &Edge) -> Verdict {
    if edge.from_dir == edge.to_dir {
        return Verdict::Within;
    }
    match pairs.rule_for(&edge.from_dir, &edge.to_dir) {
        Some(rule) => Verdict::Allowed(rule.name.clone()),
        None => Verdict::Refused,
    }
}

/// The findings for a set of edges and the configuration they were judged under. `declared` is the
/// set of source directories, where the arm knows it; a rule naming any other directory guards
/// nothing and is a finding.
#[must_use]
pub fn findings(lint: &'static str, pairs: &Pairs, declared: Option<&BTreeSet<String>>, crates_read: usize, edges: &[Edge]) -> Vec<LintError> {
    let mut out = Vec::new();
    for name in &pairs.malformed {
        out.push(LintError::error("config".into(), 1, lint, format!("rule `{name}` needs both `from` and `to`, each a source directory")));
    }
    if let Some(declared) = declared {
        for rule in &pairs.rules {
            for end in [&rule.from, &rule.to] {
                if !declared.contains(end) {
                    out.push(LintError::error("config".into(), 1, lint, format!(
                        "rule `{}` names `{end}`, which is no declared source directory ({}), so it allows nothing",
                        rule.name,
                        declared.iter().cloned().collect::<Vec<_>>().join(", ")
                    )));
                }
            }
        }
    }
    if crates_read == 0 {
        out.push(LintError::error("config".into(), 1, lint, "read no crate under the declared source directories, so no edge was judged: a pass here would be a pass over nothing".into()));
    }
    for edge in edges {
        if judge(pairs, edge) != Verdict::Refused {
            continue;
        }
        let mut e = LintError::error(edge.from_crate.clone(), edge.line, lint, format!(
            "`{}` in `{}` takes `{}` in `{}` ({}), and no rule allows an edge from `{}` onto `{}`",
            edge.from_crate, edge.from_dir, edge.to_crate, edge.to_dir, edge.section, edge.from_dir, edge.to_dir
        ));
        e.path = edge.manifest.clone();
        out.push(e);
    }
    out
}

// -------------------------------------------------------------------------------------------------
// Arm A: the repo lint, reading the manifests under the declared directories through the view
// -------------------------------------------------------------------------------------------------

/// One package found under a source directory.
#[derive(Clone, Debug)]
pub struct Package {
    pub name:     String,
    pub dir:      String,
    pub manifest: PathBuf,
}

/// The declared directories as `src_dirs` spells them, relative to the mock dir and without a
/// trailing slash, each with its absolute path. Read through `SrcLayout`, the one place the shipped
/// lints turn the engine's absolute directories back into the spelling the config uses.
#[must_use]
pub fn declared_names(mock: &Path, src_dirs: &[PathBuf]) -> Vec<(String, PathBuf)> {
    SrcLayout::new(mock, src_dirs).dirs().iter().map(|d| (d.clone(), mock.join(d))).collect()
}

/// Every package directly under each declared directory: a directory holding a `Cargo.toml` with a
/// `[package]` table. Named by `package.name`, the name a dependency key uses.
#[must_use]
pub fn packages(view: &View<'_>, mock: &Path, src_dirs: &[PathBuf]) -> Vec<Package> {
    let mut out = Vec::new();
    for (rel, abs) in declared_names(mock, src_dirs) {
        let Some(listing) = view.list(&abs) else { continue };
        for entry in listing.entries() {
            if !entry.is_dir() {
                continue;
            }
            let manifest = abs.join(entry.name()).join("Cargo.toml");
            let Some(Ok(table)) = view.toml(&manifest) else { continue };
            let Some(package) = table.get("package").and_then(toml::Value::as_table) else { continue };
            let name = package.get("name").and_then(toml::Value::as_str).unwrap_or(entry.name()).to_owned();
            out.push(Package { name, dir: rel.clone(), manifest });
        }
    }
    out
}

/// The dependency tables of one manifest: the three top-level ones and each `[target.*]` copy.
fn dependency_tables(manifest: &toml::Table) -> Vec<(String, &toml::Table)> {
    let mut out = Vec::new();
    for section in SECTIONS {
        if let Some(t) = manifest.get(section).and_then(toml::Value::as_table) {
            out.push((section.to_owned(), t));
        }
    }
    if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
        for (cfg, body) in targets {
            for section in SECTIONS {
                if let Some(t) = body.get(section).and_then(toml::Value::as_table) {
                    out.push((format!("target.{cfg}.{section}"), t));
                }
            }
        }
    }
    out
}

/// The 1-based line naming `key` in a dependency table, either `key = ...`, `key.workspace = ...` or a
/// `[...dependencies.key]` header; 1 where none is found.
fn line_of(text: &str, key: &str) -> usize {
    for (n, line) in text.lines().enumerate() {
        let t = line.trim();
        let entry = t.strip_prefix(key).is_some_and(|rest| rest.starts_with([' ', '.', '=']));
        let header = t.starts_with('[') && t.ends_with(&format!(".{key}]"));
        if entry || header {
            return n + 1;
        }
    }
    1
}

/// Every edge between two packages found under the declared directories.
#[must_use]
pub fn manifest_edges(view: &View<'_>, repo_root: &Path, packages: &[Package]) -> Vec<Edge> {
    let by_name: BTreeMap<&str, &Package> = packages.iter().map(|p| (p.name.as_str(), p)).collect();
    let mut out = Vec::new();
    for p in packages {
        let Some(Ok(manifest)) = view.toml(&p.manifest) else { continue };
        let text = view.text(&p.manifest).unwrap_or("");
        let rel = p.manifest.strip_prefix(repo_root).unwrap_or(&p.manifest).to_string_lossy().into_owned();
        for (section, table) in dependency_tables(manifest) {
            for (key, value) in table {
                let target = value.get("package").and_then(toml::Value::as_str).unwrap_or(key);
                let Some(to) = by_name.get(target) else { continue };
                out.push(Edge {
                    from_crate: p.name.clone(),
                    from_dir:   p.dir.clone(),
                    to_crate:   to.name.clone(),
                    to_dir:     to.dir.clone(),
                    section:    section.clone(),
                    manifest:   Some(rel.clone()),
                    line:       line_of(text, key),
                });
            }
        }
    }
    out
}

/// Arm A.
#[derive(Default)]
pub struct ByManifest {
    pub pairs: Pairs,
}

impl Lint for ByManifest {
    fn name(&self) -> &'static str {
        NAME
    }

    fn description(&self) -> &'static str {
        "a dependency edge joins two source directories only where a rule allows that pair"
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }

    fn config_keys(&self) -> &[&str] {
        &["rule"]
    }

    fn configure(&mut self, params: &HashMap<String, String>) {
        self.pairs = Pairs::from_params(params);
    }
}

impl RepoLint for ByManifest {
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError> {
        let view = ctx.view();
        let src_dirs = ctx.src_dirs();
        let declared: BTreeSet<String> = declared_names(ctx.mock_dir, src_dirs).into_iter().map(|(n, _)| n).collect();
        let packages = packages(view, ctx.mock_dir, src_dirs);
        let edges = manifest_edges(view, ctx.repo_root, &packages);
        findings(NAME, &self.pairs, Some(&declared), packages.len(), &edges)
    }
}

// -------------------------------------------------------------------------------------------------
// Arm B: the workspace lint, over the crates and the dependency names the engine discovered
// -------------------------------------------------------------------------------------------------

/// The edges between the crates the engine hands a workspace lint, each crate's directory read off
/// its `crate_dir` relative to the mock dir.
#[must_use]
pub fn engine_edges(crates: &[(&str, &LintContext)]) -> Vec<Edge> {
    let dir_of = |ctx: &LintContext| -> String {
        let parent = ctx.crate_dir().parent().unwrap_or(ctx.crate_dir());
        parent.strip_prefix(ctx.workspace_root()).unwrap_or(parent).to_string_lossy().replace('\\', "/")
    };
    let dirs: BTreeMap<&str, String> = crates.iter().map(|(n, c)| (*n, dir_of(c))).collect();
    let mut out = Vec::new();
    for (name, ctx) in crates {
        for dep in ctx.deps() {
            let Some(to_dir) = dirs.get(dep.as_str()) else { continue };
            out.push(Edge {
                from_crate: (*name).to_owned(),
                from_dir:   dirs[name].clone(),
                to_crate:   dep.clone(),
                to_dir:     to_dir.clone(),
                section:    "the engine's deps, section not kept".into(),
                manifest:   None,
                line:       1,
            });
        }
    }
    out
}

/// Arm B.
#[derive(Default)]
pub struct ByEngineDeps {
    pub pairs: Pairs,
}

impl Lint for ByEngineDeps {
    fn name(&self) -> &'static str {
        NAME_BY_ENGINE
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }

    fn per_file(&self) -> bool {
        false
    }

    fn config_keys(&self) -> &[&str] {
        &["rule"]
    }

    fn configure(&mut self, params: &HashMap<String, String>) {
        self.pairs = Pairs::from_params(params);
    }
}

impl WorkspaceLint for ByEngineDeps {
    fn check_all(&self, crates: &[(&str, &LintContext)]) -> Vec<LintError> {
        let edges = engine_edges(crates);
        findings(NAME_BY_ENGINE, &self.pairs, None, crates.len(), &edges)
    }
}
