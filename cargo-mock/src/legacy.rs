//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The pin a repo has that never declared one.
//!
//! Before `mockspace_version` existed, the engine was an ordinary git
//! dependency of the mock workspace and its revision was whatever `Cargo.lock`
//! had resolved. Reading that keeps such a repo running until somebody adds the
//! explicit key, and it is what seeds the key when they do.
//!
//! Nothing writes this form any more. It is read-only and it goes when the last
//! repo carrying one has migrated.

use std::path::Path;

use renki::{Pin, Reference};
use serde::Deserialize;

/// The engine revision recorded in `dir`'s `Cargo.lock`, with the url from the
/// same entry. `None` when there is no lock, it does not parse, or the engine
/// is not a git dependency in it.
pub fn pin_from_lock_at(dir: &Path) -> Option<Pin> {
    let text = std::fs::read_to_string(dir.join("Cargo.lock")).ok()?;
    pin_from_lock(&text)
}

fn pin_from_lock(lock_text: &str) -> Option<Pin> {
    let lock: CargoLock = toml::from_str(lock_text).ok()?;
    let source = lock
        .package
        .into_iter()
        .find(|p| p.name == "mockspace")
        .and_then(|p| p.source)?;
    // `git+<url>[?query]#<rev>`
    let locator = source.strip_prefix("git+").unwrap_or(&source);
    let (url_part, rev) = locator.rsplit_once('#')?;
    Some(Pin {
        url:       url_part.split('?').next().unwrap_or(url_part).to_string(),
        reference: Reference::Rev(rev.to_string()),
    })
}

#[derive(Deserialize)]
struct CargoLock {
    #[serde(default)]
    package: Vec<LockPackage>,
}

#[derive(Deserialize)]
struct LockPackage {
    name:   String,
    #[serde(default)]
    source: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCK: &str = "\
[[package]]
name = \"other\"
version = \"1.0\"

[[package]]
name = \"mockspace\"
version = \"0.1.0\"
source = \"git+ssh://git@github.com/hiisi-digital/mockspace.git?branch=dev#deadbeef1234\"
";

    #[test]
    fn the_query_string_is_dropped_and_the_fragment_is_the_rev() {
        let pin = pin_from_lock(LOCK).unwrap();
        assert_eq!(pin.url, "ssh://git@github.com/hiisi-digital/mockspace.git");
        assert_eq!(pin.reference, Reference::Rev("deadbeef1234".into()));
    }

    #[test]
    fn anything_that_is_not_a_git_engine_entry_is_no_pin() {
        // a lock without the engine at all
        assert!(pin_from_lock("[[package]]\nname = \"other\"\nversion = \"1.0\"\n").is_none());
        // the engine as a registry dependency, which carries no rev
        assert!(
            pin_from_lock(
                "[[package]]\nname = \"mockspace\"\nversion = \"1\"\nsource = \
                 \"registry+https://github.com/rust-lang/crates.io-index\"\n"
            )
            .is_none()
        );
        // a path dependency, which has no `source` key at all
        assert!(pin_from_lock("[[package]]\nname = \"mockspace\"\nversion = \"1\"\n").is_none());
        // and a file that is not TOML
        assert!(pin_from_lock("{\"not\": \"toml\"").is_none());
    }

    #[test]
    fn the_lock_is_read_from_the_directory_it_is_given() {
        let d = tempfile::tempdir().unwrap();
        assert!(
            pin_from_lock_at(d.path()).is_none(),
            "control: an empty directory declares nothing"
        );
        std::fs::write(d.path().join("Cargo.lock"), LOCK).unwrap();
        assert_eq!(
            pin_from_lock_at(d.path()).unwrap().reference,
            Reference::Rev("deadbeef1234".into())
        );
    }
}
