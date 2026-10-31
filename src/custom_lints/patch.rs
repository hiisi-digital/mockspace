//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The `[patch]` that unifies a pack's `mockspace-lint-rules` with the engine's own revision, and
//! the lookup of cargo's checkout of it.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::write_if_changed;


/// Build the `[patch]` that pins `mockspace-lint-rules` to the engine's exact
/// lint-rules, so a `[lint-crates]` pack that references lint-rules by branch
/// unifies with the engine's rev-pinned copy instead of building a second one.
///
/// The patch points at a **path**: cargo's own checkout of the engine's source
/// at that rev (`$CARGO_HOME/git/checkouts/<name>-<hash>/<short-rev>/lint-rules`),
/// which cargo just extracted to build the engine. It has to be a path (or any
/// source other than the original git url) because cargo rejects a patch that
/// points back at the same git source. `None` when the dep is not a git-rev spec
/// (a tag/version or a local path override) or the checkout cannot be located,
/// in which case no patch is emitted.
pub(super) fn patch_section(lint_rules_dep: &str, cargo_home: &Path) -> Option<String> {
    let url = extract_between(lint_rules_dep, "git = \"", "\"")?;
    let rev = extract_between(lint_rules_dep, "rev = \"", "\"")?;
    let path = find_lint_rules_checkout(cargo_home, &url, &rev)?;
    let mut out = String::new();
    for spelling in spellings_of(&url) {
        out.push_str(&format!(
            "\n[patch.\"{spelling}\"]\nmockspace-lint-rules = {{ path = \"{}\" }}\n",
            path.display()
        ));
    }
    Some(out)
}

/// Every url a pack might have written for the same repository.
///
/// Cargo keys a source on the url string, so `https://github.com/o/r.git` and
/// `ssh://git@github.com/o/r.git` are two sources holding two copies of one
/// crate, and a patch on one covers neither the other nor anything that named
/// it. That is the exact failure the patch above exists to prevent, arriving
/// through the one door it did not cover.
///
/// It is not hypothetical and it is not rare: the engine names its own
/// lint-rules by whichever spelling its manifest carries, and a `[lint-crates]`
/// pack in somebody else's repository names it by whichever spelling that
/// author wrote. Both are correct and they do not have to agree.
///
/// So the patch is emitted for both, which costs a table cargo ignores when
/// nothing names that source and is what makes the unification hold when
/// something does.
///
/// Only these two forms, and no attempt to normalise beyond them: a url this
/// does not recognise is passed through unchanged, so a self-hosted forge or a
/// scheme nobody here uses behaves exactly as it did before.
///
/// A `.git`-less spelling is deliberately not a third form. Cargo canonicalises
/// that suffix away, so a patch key differing from the dependency url only by
/// it already matches, while a key naming a different repository does not.
/// Scheme and userinfo are what canonicalisation preserves, which is why this
/// is the axis and the only one.
///
/// **The answers are distinct**, and that is load-bearing rather than tidy: two
/// identical `[patch."X"]` tables is `error: duplicate key`, which cargo raises
/// at manifest parse before resolving anything, so every `cargo mock` in that
/// repository would stop. The dedup is what makes a future third form safe to
/// add without knowing this.
fn spellings_of(url: &str) -> Vec<String> {
    let (https, ssh) = if let Some(rest) = url.strip_prefix("https://") {
        (url.to_string(), format!("ssh://git@{rest}"))
    } else if let Some(rest) = url.strip_prefix("ssh://git@") {
        (format!("https://{rest}"), url.to_string())
    } else {
        return vec![url.to_string()];
    };
    let mut out = vec![https];
    if !out.contains(&ssh) {
        out.push(ssh);
    }
    out
}

/// The throwaway package the fetch runs against: lint-rules and nothing else.
///
/// A workspace of its own, so it does not join the generated crate's, and a lib
/// target, so cargo has a package rather than a manifest it refuses to read.
fn fetch_manifest(lint_rules_dep: &str) -> String {
    format!(
        "[workspace]\n\n\
         [package]\nname = \"lint-rules-fetch\"\nversion = \"0.0.0\"\n\
         edition = \"2024\"\npublish = false\n\n\
         [lib]\npath = \"lib.rs\"\n\n\
         [dependencies]\nmockspace = {lint_rules_dep}\n"
    )
}

/// Whether the dependency names a git repository at a fixed revision, which is
/// the only shape [`patch_section`] can build anything from.
/// The url has to carry a scheme, since cargo refuses a relative one and the
/// fetch would spend a process to be told so.
pub(super) fn is_git_rev_dep(dep: &str) -> bool {
    extract_between(dep, "git = \"", "\"").is_some_and(|u| u.contains("://"))
        && extract_between(dep, "rev = \"", "\"").is_some()
}

/// Make cargo extract its checkout of `lint-rules` at the engine's revision, so
/// [`patch_section`] has something to point at.
///
/// The patch is built from a path into cargo's checkout, and on a cold cache
/// that checkout does not exist at the moment the manifest is written: cargo
/// extracts it while building this crate, which is afterwards. So the first run
/// in a repository emitted no patch at all, resolved a second copy of
/// `mockspace-lint-rules` for every pack or tool naming it by another spelling,
/// and failed with `E0308` between `LintPack` and `LintPack` at one file and
/// one line, with nothing in the output naming a url. The next run, the
/// checkout now present, worked.
///
/// That is the worst shape a defect takes, because it repairs itself and the
/// repair looks like the thing having been flaky. Observed in `muisti`, where
/// four tools were rewritten to a different url spelling on the strength of it.
///
/// What is fetched is a throwaway package depending on lint-rules and nothing
/// else, rather than the generated crate, since that one is not written yet and
/// its manifest is what this is being computed for. Failure is silent on
/// purpose: the caller asks [`patch_section`] again either way, so a fetch that
/// could not run costs the patch and nothing more, which is where it was.
pub(super) fn ensure_lint_rules_checkout(gen_dir: &Path, lint_rules_dep: &str, cargo_home: &Path) {
    let dir = gen_dir.join(".fetch");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    if std::fs::write(dir.join("lib.rs"), "").is_err() {
        return;
    }
    if write_if_changed(&dir.join("Cargo.toml"), &fetch_manifest(lint_rules_dep)).is_err() {
        return;
    }
    // stderr is inherited: this is where cargo says it is updating a git
    // repository, which is the only thing on screen explaining a cold run's
    // pause. stdout carries nothing worth reading here.
    //
    // `CARGO_HOME` is passed rather than inherited, so the fetch writes its
    // checkout under the same root [`patch_section`] then reads. Inheriting it
    // agrees with that root in every real run and in none that plants one.
    let _ = Command::new("cargo")
        .arg("fetch")
        .arg("--manifest-path")
        .arg(dir.join("Cargo.toml"))
        .env("CARGO_HOME", cargo_home)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status();
}

/// Locate cargo's checkout of `lint-rules` at `rev` for the git repo `url`,
/// under `<cargo_home>/git/checkouts/<name>-<hash>/<short-rev>/lint-rules`. The
/// `<name>-<hash>` dir is keyed on the url; the sub-dir is the short rev.
fn find_lint_rules_checkout(cargo_home: &Path, url: &str, rev: &str) -> Option<PathBuf> {
    let name = url.rsplit('/').next()?.trim_end_matches(".git");
    let short = &rev[.. rev.len().min(7)];
    let checkouts = cargo_home.join("git").join("checkouts");
    for repo in std::fs::read_dir(&checkouts).ok()?.flatten() {
        if !repo
            .file_name()
            .to_string_lossy()
            .starts_with(&format!("{name}-"))
        {
            continue;
        }
        for co in std::fs::read_dir(repo.path()).ok()?.flatten() {
            if co.file_name().to_string_lossy().starts_with(short) {
                let lr = co.path().join("lint-rules");
                if lr.is_dir() {
                    return Some(lr);
                }
            }
        }
    }
    None
}

/// `$CARGO_HOME` or `~/.cargo`.
pub(super) fn cargo_home() -> PathBuf {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))
        .unwrap_or_else(|| PathBuf::from(".cargo"))
}

/// The substring of `s` between the first `start` and the next `end` after it.
pub(super) fn extract_between(s: &str, start: &str, end: &str) -> Option<String> {
    let from = s.find(start)? + start.len();
    let rest = &s[from ..];
    let to = rest.find(end)?;
    Some(rest[.. to].to_string())
}

#[cfg(test)]
#[path = "patch_tests.rs"]
mod tests;
