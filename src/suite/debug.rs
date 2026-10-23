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
//! needs no symbols is named lean too. No manifest is edited and no cargo config
//! is, and neither is any input of a member's fingerprint.
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
        if self.off {
            return Some(
                "    debug    off (--no-debug): no crate gets symbols, a failed one included"
                    .to_string(),
            );
        }
        if !self.lean {
            return None;
        }
        if self.everywhere {
            return Some("    debug    symbols for every crate (--debug)".to_string());
        }
        if self.crates.is_empty() {
            return Some(
                "    debug    no crate needs symbols; every crate is built without debug info"
                    .to_string(),
            );
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
            "    debug    symbols for {}; every other crate is built without debug info",
            each.join(", ")
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

/// What the cargo configs in `dirs` and the environment set that gives debug
/// info to the `profiles`. Read only: nothing is edited.
///
/// A config file that does not parse, or is not there, says nothing, which is
/// what cargo's own message about it is for.
pub fn settings_in_the_way(
    dirs: &[PathBuf],
    profiles: &[&str],
    env_set: &dyn Fn(&str) -> bool,
) -> Vec<Found> {
    let mut found = Vec::new();
    for dir in dirs {
        // `config.toml`, else the older `config`, as cargo reads them.
        let Some((path, text)) = ["config.toml", "config"].iter().find_map(|n| {
            let p = dir.join(n);
            std::fs::read_to_string(&p).ok().map(|t| (p, t))
        }) else {
            continue;
        };
        let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
            continue;
        };
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

/// What the run says about those settings: one note for each file, and one for
/// the environment, said only where the run builds lean.
pub fn notes(found: &[Found]) -> Vec<String> {
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
            format!(
                "note: {who} {}; `mock test` builds over it without debug info and gives \
                 symbols only to the crates that need them, and edits nothing. \
                 `[test] lean = false` lets it stand.",
                what.join(" and ")
            )
        })
        .collect()
}

/// What a run says about debug info before it builds: each cargo setting that
/// would have given every crate some, where the run builds lean over it, a
/// `--config` of the caller's that comes after this run's and wins, and which
/// crates get symbols.
pub fn report(
    s: &TestSettings,
    mock_dir: &Path,
    profile: &str,
    symbols: &Symbols,
    passed: &[String],
) {
    // Lean is applied under `dev` only, and a clone that is not lean has handed
    // the profile back, so neither has anything overridden to report.
    if s.lean && profile == "dev" {
        let home = std::env::var_os("CARGO_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")));
        let dirs = config_dirs(mock_dir, home);
        let set_in_env = |v: &str| std::env::var_os(v).is_some();
        for n in notes(&settings_in_the_way(&dirs, &["dev", "test"], &set_in_env)) {
            eprintln!("{n}");
        }
        if passes_own_debug(passed) {
            eprintln!(
                "note: a `--config` passed on the command line sets debug info and comes after \
                 this run's own, so it wins for what it names."
            );
        }
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
