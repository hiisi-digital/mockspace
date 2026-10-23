//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The cargo config one run is built with, passed with `--config` and written
//! fresh under the build directory, so nothing in the consumer's manifest or in
//! anybody's cargo config is edited.
//!
//! It carries two things: each member's opt level, from `[test.opt_level]`, and
//! debug info ([`super::debug`]): none anywhere except the crates that need it.
//! The opt levels are an input of every member's fingerprint and the debug info
//! is not, since a test passes or fails the same with or without symbols; so
//! the fingerprint is taken over [`render`] without symbols and the run is
//! built with it with them.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::debug::Symbols;
use super::settings::{OptLevel, TestSettings};

/// What one member's tables say.
#[derive(Default)]
struct Entry {
    opt:   Option<OptLevel>,
    /// The value of `debug`, as it is written.
    debug: Option<String>,
}

/// The cargo config for one run, under the profile the run builds. `None` when
/// it would say nothing.
///
/// An entry naming something that is not among `members` is left out, since
/// cargo warns about a profile spec that matches no package on every build;
/// `members` is `None` where the workspace could not be read, and then every
/// opt level stays and no member is named lean.
///
/// **Debug info is set under `dev` only**, and for both `dev` and `test`, since
/// `cargo test` builds under `test`, which an explicit `[profile.test]` of a
/// manifest or a cargo config keeps from inheriting `dev`'s. A `release` or
/// custom profile is what the caller chose and is left as it is, apart from the
/// crates that need symbols.
pub fn render(
    s: &TestSettings,
    profile: &str,
    members: Option<&[String]>,
    symbols: Option<&Symbols>,
) -> Option<String> {
    let is_member = |m: &String| members.is_none_or(|ms| ms.contains(m));
    let lean = symbols.is_some_and(|y| y.lean) && profile == "dev";
    let everywhere = lean && symbols.is_some_and(|y| y.everywhere);
    let mut entries: BTreeMap<&String, Entry> = BTreeMap::new();
    for (m, level) in s.opt_level.iter().filter(|(m, _)| is_member(m)) {
        entries.entry(m).or_default().opt = Some(*level);
    }
    if let Some(y) = symbols {
        for m in y.crates.keys() {
            entries.entry(m).or_default().debug = Some(y.level.toml());
        }
        // A member that needs none is named lean as well, since a manifest's
        // entry for one crate beats a profile-wide setting from a config.
        if lean && !everywhere {
            for m in members.unwrap_or_default() {
                entries
                    .entry(m)
                    .or_default()
                    .debug
                    .get_or_insert_with(|| "0".to_string());
            }
        }
    }
    if !lean && entries.is_empty() {
        return None;
    }

    // Without symbols, the text a fingerprint is taken over, which must stay
    // what it was or every recorded pass would stop standing.
    let mut t = String::from(if symbols.is_none() {
        "# Written by `cargo mock test` for each run, from `[test.opt_level]` in\n\
         # mockspace.toml. Edits here are lost.\n"
    } else {
        "# Written by `cargo mock test` for each run, from `[test]` in mockspace.toml:\n\
         # `opt_level`, and the debug info the crates that need symbols get.\n\
         # Edits here are lost.\n"
    });
    let debug_profiles: &[&str] = if profile == "dev" { &["dev", "test"] } else { &[profile] };
    if lean {
        let top = if everywhere {
            symbols.map_or_else(|| "0".to_string(), |y| y.level.toml())
        } else {
            "0".to_string()
        };
        for p in debug_profiles {
            let _ = write!(t, "\n[profile.{p}]\ndebug = {top}\n");
        }
    }
    for (member, e) in &entries {
        if e.opt.is_some() || e.debug.is_some() {
            let _ = writeln!(t, "\n[profile.{profile}.package.\"{member}\"]");
        }
        if let Some(level) = e.opt {
            if level.is_word() {
                let _ = writeln!(t, "opt-level = \"{level}\"");
            } else {
                let _ = writeln!(t, "opt-level = {level}");
            }
        }
        if let Some(d) = &e.debug {
            let _ = writeln!(t, "debug = {d}");
        }
    }
    // `test` beside `dev`, for the debug info only.
    if profile == "dev" {
        for (member, e) in &entries {
            if let Some(d) = &e.debug {
                let _ = write!(t, "\n[profile.test.package.\"{member}\"]\ndebug = {d}\n");
            }
        }
    }
    Some(t)
}

#[cfg(test)]
#[path = "cargo_config_tests.rs"]
mod tests;
