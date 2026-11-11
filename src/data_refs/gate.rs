//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The lints ahead of a commit of a data ref.
//!
//! What a commit of a data ref takes out of the instance's changes, the phase gates no longer
//! see, and the commit passes through no hook of the branch. So the repository lints are asked
//! first, as they are before a commit of the branch, and nothing is committed where they refuse.

use std::path::Path;
use std::process::ExitCode;

/// Whether a `data-refs` call writes a ref: `commit`, the one verb that takes the instance's
/// changes out of view by putting them on the ref.
fn writes_a_data_ref(verb_args: &[&str]) -> bool {
    verb_args.first() == Some(&"commit")
}

/// Runs the repository lints, which the phase gates are among, the way a commit of the branch
/// does, and says whether they pass.
pub fn repo_lints_pass(repo_root: &Path) -> bool {
    std::process::Command::new("cargo")
        .args(["mock", "--lint-only", "--scope", "infra"])
        .current_dir(repo_root)
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// `cargo mock data-refs ..` as the launcher runs it: a verb that writes a ref runs only where
/// `lints` passes over the repository, and every other verb runs as it is.
pub fn run_gated(cfg: &crate::config::Config, args: &[&str], lints: &dyn Fn(&Path) -> bool) -> ExitCode {
    if writes_a_data_ref(args) && !lints(&cfg.repo_root) {
        eprintln!(
            "data-refs: the lints refuse this tree, so nothing was committed to a ref; `cargo mock --lint-only \
             --scope infra` names why"
        );
        return ExitCode::FAILURE;
    }
    super::run(cfg, args)
}

#[cfg(test)]
mod tests {
    use super::writes_a_data_ref;

    #[test]
    fn commit_is_the_one_verb_the_lints_are_asked_before() {
        assert!(writes_a_data_ref(&["commit"]) && writes_a_data_ref(&["commit", "research"]));
        for verb in ["status", "verify", "instantiate", "update", "restore", "trailers", "paired"] {
            assert!(!writes_a_data_ref(&[verb]), "`data-refs {verb}` was gated as a write");
        }
    }
}
