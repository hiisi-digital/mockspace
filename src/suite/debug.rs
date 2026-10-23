//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Which crates a test build gives debug info to, and what the run says about
//! the cargo settings that would have given it to all of them.
//!
//! **A test build is lean and symbols are added where one is needed to trace a
//! problem.** Debug info is the larger part of what a test build writes. A crate
//! needs it when something in it is being traced, which is two cases: its tests failed the last time they ran
//! ([`super::failed`]), or the caller named it with `--debug`.
//!
//! **How it reaches cargo** is the generated `--config` file
//! ([`super::cargo_config`]), a profile-wide `debug = 0` and a
//! `[profile.<p>.package."<crate>"]` entry for each crate that needs symbols.
//! Measured against cargo 1.98 nightly and 1.94 stable: a `--config` file beats
//! a cargo config file (`$CARGO_HOME/config.toml`, `.cargo/config.toml`), the
//! environment and the manifest, per key; the environment has no spelling for a
//! single package (`CARGO_PROFILE_<P>_PACKAGE_<NAME>_DEBUG` is read as nothing, in
//! any case and with `-` as `_`); and a manifest's `[profile.<p>.package.<name>]`
//! beats a profile-wide setting from a config, which is why every member that
//! needs no symbols is named lean too. For the packages outside the workspace the
//! same holds with `"*"` for all of them, which beats a profile-wide setting
//! from a manifest or a config and is beaten by this run's own, and a name beats
//! a `"*"`, so each one a manifest or a config names for itself is named lean as
//! well ([`pinned_packages`]). All of it is pinned by
//! `tests/cargo_debug_info_precedence.rs`. No manifest is edited and no cargo
//! config is, and neither is any input of a member's fingerprint.
//!
//! **What no profile setting reaches is rustflags**: `-g` or `-C debuginfo=` in
//! them gives every crate debug info. They are reported ([`rustflags_with_debug`])
//! and left, and the plan stops saying the rest is lean.
//!
//! **What building a crate with symbols costs** is that crate and the crates that
//! depend on it, once: a dependency's profile is in a dependent's fingerprint.
//! Its dependencies and every crate that is neither are not compiled again, and
//! the lean build stays in the target directory beside it, so going back
//! compiles nothing. A run of the crates that failed builds no dependents.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::args::DebugAsk;
use super::settings::{DebugLevel, TestSettings};

/// Why a crate is built with symbols.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The caller named it, with `--debug` or `--debug=<crate>`.
    Named,
    /// Its tests failed the last time they ran.
    Failed,
}

/// What a run builds with symbols.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbols {
    /// Whether every crate not named here is built without debug info.
    pub lean:       bool,
    /// The debug info a crate named here gets.
    pub level:      DebugLevel,
    /// Every crate, where a bare `--debug` met a workspace that could not be
    /// read, so no crate could be named.
    pub everywhere: bool,
    /// `--no-debug`, which is reported so a failed crate left lean is not a surprise.
    pub off:        bool,
    pub crates:     BTreeMap<String, Reason>,
    /// Packages outside the workspace that a manifest or a cargo config gives
    /// a `debug` of their own, which each need naming lean to be lean.
    pub pinned:     BTreeSet<String>,
    /// Whether rustflags give every crate debug info, which no profile setting
    /// undoes.
    pub rustflags:  bool,
}

/// What [`decide`] reads.
pub struct Basis<'a> {
    pub ask:      &'a DebugAsk,
    /// The crates named with `-p`.
    pub packages: &'a [String],
    /// The workspace's members, `None` where it could not be read.
    pub members:  Option<&'a [String]>,
    /// The members this run builds tests for, `None` where that is not known,
    /// as under plain `cargo test`.
    pub running:  Option<&'a [String]>,
    /// The members whose tests failed last time.
    pub failed:   &'a BTreeSet<String>,
}

/// The crates that need symbols this run, and everything else about how they
/// are given.
pub fn decide(s: &TestSettings, b: &Basis) -> Symbols {
    let mut out = Symbols {
        lean:       s.lean,
        level:      s.debug,
        everywhere: false,
        off:        b.ask.off,
        crates:     BTreeMap::new(),
        pinned:     BTreeSet::new(),
        rustflags:  false,
    };
    if b.ask.off {
        return out;
    }
    let member = |m: &String| b.members.is_none_or(|ms| ms.contains(m));
    let runs = |m: &String| b.running.is_none_or(|r| r.contains(m));
    for n in b.ask.names.iter().filter(|n| member(n)) {
        out.crates.insert(n.clone(), Reason::Named);
    }
    if b.ask.bare {
        if !b.packages.is_empty() {
            for p in b.packages.iter().filter(|p| member(p)) {
                out.crates.insert(p.clone(), Reason::Named);
            }
        } else if let Some(all) = b.running.or(b.members) {
            for m in all {
                out.crates.insert(m.clone(), Reason::Named);
            }
        } else {
            out.everywhere = true;
        }
    }
    for m in b.failed.iter().filter(|m| member(m) && runs(m)) {
        out.crates.entry(m.clone()).or_insert(Reason::Failed);
    }
    out
}

impl Symbols {
    /// The line the plan prints for this, `None` where there is nothing to say.
    pub fn line(&self) -> Option<String> {
        // What the lean build claims, which rustflags that give debug info to
        // every crate make untrue.
        let claim = |text: &str| if self.rustflags { String::new() } else { text.to_string() };
        if self.off {
            return Some(format!(
                "    debug    off (--no-debug): this run gives no crate symbols{}",
                claim(", a failed one included")
            ));
        }
        if !self.lean {
            return None;
        }
        if self.everywhere {
            return Some("    debug    symbols for every crate (--debug)".to_string());
        }
        if self.crates.is_empty() {
            return Some(format!(
                "    debug    no crate needs symbols{}",
                claim("; every crate is built without debug info")
            ));
        }
        let each: Vec<String> = self
            .crates
            .iter()
            .map(|(c, why)| {
                let why = match why {
                    Reason::Named => "asked for",
                    Reason::Failed => "failed last run",
                };
                format!("{c} ({why})")
            })
            .collect();
        Some(format!(
            "    debug    symbols for {}{}",
            each.join(", "),
            claim("; every other crate is built without debug info")
        ))
    }
}

/// A cargo setting that gives debug info, found where this run would have
/// built with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The file it is in, `None` where it is the environment.
    pub file:  Option<PathBuf>,
    /// The key: `profile.dev.debug`, or `profile.dev.package.x.debug`, or the
    /// variable's name.
    pub key:   String,
    /// Its value, never read from the environment.
    pub value: Option<String>,
}

/// The cargo config directories cargo reads from `from`: each `.cargo` up the
/// tree, then `CARGO_HOME`.
pub fn config_dirs(from: &Path, cargo_home: Option<PathBuf>) -> Vec<PathBuf> {
    let from = std::fs::canonicalize(from).unwrap_or_else(|_| from.to_path_buf());
    let mut out: Vec<PathBuf> = from.ancestors().map(|d| d.join(".cargo")).collect();
    if let Some(h) = cargo_home
        && !out.contains(&h)
    {
        out.push(h);
    }
    out
}

/// Every cargo config file in `dirs`, `config.toml` else the older `config`, as
/// cargo reads them, with its path. One that is not there or does not parse
/// says nothing, which is what cargo's own message about it is for.
fn config_docs(dirs: &[PathBuf]) -> Vec<(PathBuf, toml_edit::DocumentMut)> {
    dirs.iter()
        .filter_map(|dir| {
            ["config.toml", "config"].iter().find_map(|n| {
                let p = dir.join(n);
                let doc = std::fs::read_to_string(&p).ok()?.parse().ok()?;
                Some((p, doc))
            })
        })
        .collect()
}

/// What the cargo configs in `dirs` and the environment set that gives debug
/// info to the `profiles`. Read only: nothing is edited.
pub fn settings_in_the_way(
    dirs: &[PathBuf],
    profiles: &[&str],
    env_set: &dyn Fn(&str) -> bool,
) -> Vec<Found> {
    let mut found = Vec::new();
    for (path, doc) in config_docs(dirs) {
        for p in profiles {
            let Some(profile) = doc.get("profile").and_then(|t| t.get(p)) else {
                continue;
            };
            if let Some(v) = profile.get("debug") {
                found.push(Found {
                    file:  Some(path.clone()),
                    key:   format!("profile.{p}.debug"),
                    value: Some(v.to_string().trim().to_string()),
                });
            }
            if let Some(pkgs) = profile.get("package").and_then(|t| t.as_table_like()) {
                for (name, t) in pkgs.iter() {
                    if let Some(v) = t.get("debug") {
                        found.push(Found {
                            file:  Some(path.clone()),
                            key:   format!("profile.{p}.package.{name}.debug"),
                            value: Some(v.to_string().trim().to_string()),
                        });
                    }
                }
            }
        }
    }
    for p in profiles {
        let var = format!("CARGO_PROFILE_{}_DEBUG", p.to_uppercase().replace('-', "_"));
        if env_set(&var) {
            found.push(Found {
                file:  None,
                key:   var,
                value: None,
            });
        }
    }
    found
}

/// The packages outside the workspace that a manifest or a cargo config gives
/// a `debug` setting of their own, in any of the `profiles`.
///
/// A name beats both a profile-wide setting and the `"*"` this run writes, so
/// each is named lean in the run's config too. A member is not one: every
/// member is named already. `"*"` is not one either, since the run's own
/// beats it. `members` is `None` where the workspace could not be read, and
/// then every name stays.
pub fn pinned_packages(
    dirs: &[PathBuf],
    manifest: &Path,
    profiles: &[&str],
    members: Option<&[String]>,
) -> BTreeSet<String> {
    let mut docs: Vec<toml_edit::DocumentMut> =
        config_docs(dirs).into_iter().map(|(_, d)| d).collect();
    if let Some(m) = std::fs::read_to_string(manifest)
        .ok()
        .and_then(|t| t.parse().ok())
    {
        docs.push(m);
    }
    let mut out = BTreeSet::new();
    for doc in &docs {
        for p in profiles {
            let pkgs = doc
                .get("profile")
                .and_then(|t| t.get(p))
                .and_then(|t| t.get("package"))
                .and_then(|t| t.as_table_like());
            for (name, t) in pkgs.into_iter().flat_map(|t| t.iter()) {
                if name != "*"
                    && t.get("debug").is_some()
                    && members.is_none_or(|ms| !ms.iter().any(|m| m == name))
                {
                    out.insert(name.to_string());
                }
            }
        }
    }
    out
}

/// Whether these rustflags give debug info: `-g`, or `-C debuginfo=` at
/// anything but none, in either spelling.
fn gives_debug_info(flags: &[String]) -> bool {
    let adds = |v: &str| !matches!(v, "0" | "none");
    let mut it = flags.iter().map(String::as_str);
    while let Some(f) = it.next() {
        if f == "-g" {
            return true;
        }
        let value = match f {
            "-C" => it.next().and_then(|v| v.strip_prefix("debuginfo=")),
            _ => f.strip_prefix("-Cdebuginfo="),
        };
        if value.is_some_and(adds) {
            return true;
        }
    }
    false
}

fn words(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

/// The rustflags that give debug info to every crate whatever the profile
/// says, found where cargo reads them. `env` answers for a variable, and its
/// value is only read here, never kept.
///
/// Cargo takes one source: `CARGO_ENCODED_RUSTFLAGS`, else `RUSTFLAGS`, else
/// `build.rustflags` (`CARGO_BUILD_RUSTFLAGS` is that key's environment form),
/// else the `[target.*]` ones; the configs are read only where no variable
/// is set. A `[target.*]` entry is reported without asking whether its
/// condition holds here.
pub fn rustflags_with_debug(dirs: &[PathBuf], env: &dyn Fn(&str) -> Option<String>) -> Vec<Found> {
    let from_env = [
        ("CARGO_ENCODED_RUSTFLAGS", '\u{1f}'),
        ("RUSTFLAGS", ' '),
        ("CARGO_BUILD_RUSTFLAGS", ' '),
    ];
    for (var, sep) in from_env {
        if let Some(v) = env(var) {
            let flags: Vec<String> = v.split(sep).map(str::to_string).collect();
            return if gives_debug_info(&flags) {
                vec![Found {
                    file:  None,
                    key:   var.to_string(),
                    value: None,
                }]
            } else {
                Vec::new()
            };
        }
    }
    let mut found = Vec::new();
    for (path, doc) in config_docs(dirs) {
        let mut tables: Vec<(String, &toml_edit::Item)> = Vec::new();
        if let Some(b) = doc.get("build") {
            tables.push(("build".to_string(), b));
        }
        if let Some(t) = doc.get("target").and_then(|t| t.as_table_like()) {
            tables.extend(t.iter().map(|(k, v)| (format!("target.{k}"), v)));
        }
        for (name, table) in tables {
            let Some(item) = table.get("rustflags") else {
                continue;
            };
            let flags: Vec<String> = match (item.as_str(), item.as_array()) {
                (Some(s), _) => words(s),
                (_, Some(a)) => {
                    a.iter()
                        .filter_map(|v| v.as_str())
                        .map(str::to_string)
                        .collect()
                },
                _ => Vec::new(),
            };
            if gives_debug_info(&flags) {
                found.push(Found {
                    file:  Some(path.clone()),
                    key:   format!("{name}.rustflags"),
                    value: Some(item.to_string().trim().to_string()),
                });
            }
        }
    }
    found
}

/// What the run says about those settings: one note for each file, and one for
/// the environment, said only where the run builds lean.
pub fn notes(found: &[Found]) -> Vec<String> {
    grouped(found, |who, what| {
        format!(
            "note: {who} {what}; `mock test` builds over it without debug info and gives \
             symbols only to the crates that need them, and edits nothing. \
             `[test] lean = false` lets it stand."
        )
    })
}

/// What the run says about rustflags that give debug info. They are no profile
/// setting, so nothing here overrides them.
pub fn rustflag_notes(found: &[Found]) -> Vec<String> {
    grouped(found, |who, what| {
        format!(
            "note: {who} {what}, which gives every crate debug info whatever the profile \
             says; `mock test` does not override rustflags, so none of the crates is lean."
        )
    })
}

/// One line for each file in `found`, and one for the environment, in the
/// order they were found, `say` given who and what.
fn grouped(found: &[Found], say: impl Fn(&str, &str) -> String) -> Vec<String> {
    let mut sources: Vec<(Option<&PathBuf>, Vec<String>)> = Vec::new();
    for f in found {
        let what = match &f.value {
            Some(v) => format!("{} = {v}", f.key),
            None => f.key.clone(),
        };
        match sources
            .iter_mut()
            .find(|(file, _)| *file == f.file.as_ref())
        {
            Some((_, v)) => v.push(what),
            None => sources.push((f.file.as_ref(), vec![what])),
        }
    }
    sources
        .into_iter()
        .map(|(file, what)| {
            let who = match file {
                Some(p) => format!("{} sets", p.display()),
                None => "the environment sets".to_string(),
            };
            say(&who, &what.join(" and "))
        })
        .collect()
}

/// What a run reads of the cargo settings around it before it builds: each
/// that would have given every crate some debug info, in the notes returned;
/// the packages outside the workspace that a manifest or a cargo config names
/// for debug info of their own, and whether rustflags give it to all, both
/// kept in `symbols` for the config and the plan line.
///
/// Only the lean build overrides anything. It is applied under `dev` and a
/// clone that is not lean has handed the profile back, so neither says a
/// thing is overridden. Rustflags are reported under every profile, since
/// they hold under every profile.
pub fn inspect(
    s: &TestSettings,
    mock_dir: &Path,
    profile: &str,
    passed: &[String],
    members: Option<&[String]>,
    symbols: &mut Symbols,
) -> Vec<String> {
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")));
    let dirs = config_dirs(mock_dir, home);
    let value = |v: &str| std::env::var(v).ok();
    let mut out = Vec::new();
    let flags = rustflags_with_debug(&dirs, &value);
    symbols.rustflags = !flags.is_empty();
    out.extend(rustflag_notes(&flags));
    if s.lean && profile == "dev" {
        let set_in_env = |v: &str| std::env::var_os(v).is_some();
        out.extend(notes(&settings_in_the_way(
            &dirs,
            &["dev", "test"],
            &set_in_env,
        )));
        symbols.pinned = pinned_packages(
            &dirs,
            &mock_dir.join("Cargo.toml"),
            &["dev", "test"],
            members,
        );
        if passes_own_debug(passed) {
            out.push(
                "note: a `--config` passed on the command line sets debug info and comes after \
                 this run's own, so it wins for what it names."
                    .to_string(),
            );
        }
    }
    out
}

/// Prints what [`inspect`] found and which crates get symbols.
pub fn print(notes: &[String], symbols: &Symbols) {
    for n in notes {
        eprintln!("{n}");
    }
    if let Some(line) = symbols.line() {
        println!("{line}");
    }
}

/// Whether the caller passed a `--config` that mentions a profile's debug info.
fn passes_own_debug(passed: &[String]) -> bool {
    let mut it = passed.iter();
    while let Some(a) = it.next() {
        let value = if a == "--config" {
            it.next().map(String::as_str)
        } else {
            a.strip_prefix("--config=")
        };
        if value.is_some_and(|v| v.contains("profile") && v.contains("debug")) {
            return true;
        }
    }
    false
}

#[cfg(test)]
#[path = "debug_tests.rs"]
mod tests;
