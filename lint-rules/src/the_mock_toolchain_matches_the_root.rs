//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A `rust-toolchain.toml` under the mock directory declares what the root one
//! does.
//!
//! rustup resolves the nearest file walking up from the working directory, and
//! the engine runs every gate command from the mock directory with
//! `RUSTUP_TOOLCHAIN` stripped so that file wins. So a copy there does not sit
//! beside the root pin, it replaces it, for every build, test and lint the gate
//! runs.
//!
//! The failure is silent in both directions that matter. A copy declaring
//! `channel = "nightly"` under a root pinned to a dated nightly floats the whole
//! mock workspace onto whatever rustup last installed, and nothing reports it: a
//! floating channel resolves, the build is green, and the toolchain the project
//! declared is simply not the one anything ran on. A copy declaring no
//! `components` under a root that declares them installs whatever profile the
//! machine defaults to, so a formatter gate relying on the pin to supply
//! `rustfmt` relies on a coincidence.
//!
//! The shape is not hypothetical: it was found in this workspace on more than
//! one repository at once, each with a correctly pinned root file one directory
//! above a floating copy, and each resolving a nightly months newer than the one
//! it declared. The lint is the instrument for the count, and a figure written
//! here would be true on the day it was typed and wrong after the next merge.
//!
//! **Every key the root declares, the copy declares, with the same value.** A
//! string value is equal; an array value is a superset, so the copy may add a
//! component or a target the root does not need and may not drop one. Keys the
//! root does not declare are the copy's own business.
//!
//! **A deliberate divergence says so in the file.** A
//! `# lint:allow(the-mock-toolchain-matches-the-root)` line anywhere in the copy
//! silences it, which is the whole escape hatch: two workspaces on two
//! toolchains is a real thing to want and an unwritten one is indistinguishable
//! from the accident above.
//!
//! Silent where either file is absent. No copy is the ordinary case and the one
//! this rule prefers, since then there is one file and nothing to keep in step;
//! no root file means the project pins nothing and this has no reference to
//! judge against.

use std::path::Path;

use crate::{Lint, LintError, RepoContext, RepoLint, Severity};

pub struct TheMockToolchainMatchesTheRoot;

impl Lint for TheMockToolchainMatchesTheRoot {
    fn name(&self) -> &'static str {
        "the-mock-toolchain-matches-the-root"
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }

    /// Not a source lint, so `--doc-only` does not skip it.
    ///
    /// The default is `true` and it is wrong here: this reads two toolchain
    /// files and no Rust at all, so skipping it on a doc commit skips it on
    /// most commits. Four of the five sibling repo lints declare `false` for
    /// the same reason; `canon_not_while_panel_open` declares nothing and
    /// inherits `true`, so a lint about canon edits, which are doc edits, is
    /// skipped in the mode a doc commit runs under. That one is not this
    /// change's to fix and is worth its own look.
    fn source_only(&self) -> bool {
        false
    }
}

/// A value as `rust-toolchain.toml` spells one.
///
/// Two shapes and no third, because the file's schema has no others: `channel`
/// and `profile` are strings, `components` and `targets` are arrays of strings.
/// A value of any other type is dropped rather than guessed at, so a file using
/// a shape this does not compare is not reported as disagreeing with itself.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Value {
    Str(String),
    Arr(Vec<String>),
}

/// The `[toolchain]` table, as `(key, value)` in declaration order.
///
/// A real parse, because a hand scanner is wrong here in the direction that
/// matters. The first version of this file read one line at a time and got four
/// ordinary shapes wrong: a multi-line array, a literal-string value, and
/// `[ toolchain ]` with spaces. Three of the four produced a false complaint,
/// which is noise; the fourth was silent and was the bad one. A multi-line array
/// in the ROOT file read as an absent key, so every component in it went
/// uncompared and a copy declaring none of them passed. That is the same silent
/// divergence this lint exists to catch, reproduced inside the lint.
///
/// `toml` parse-only is already in this workspace's lockfile through
/// `mockspace-manifest`, so the pack's cdylib gains no new transitive tree.
///
/// A file that is neither TOML nor rustup's legacy one-line form yields no
/// keys. That is deliberate and it is the safe direction on both sides: an
/// unreadable copy claims nothing, and an unreadable root demands nothing. A
/// malformed toolchain file is rustup's complaint to make, and making it here
/// too would report one fault twice.
fn toolchain_table(text: &str) -> Option<Vec<(String, Value)>> {
    let Ok(doc) = text.parse::<toml::Table>() else {
        return legacy_channel(text).map(|c| vec![("channel".to_string(), Value::Str(c))]);
    };
    let Some(toml::Value::Table(t)) = doc.get("toolchain") else {
        return Some(Vec::new());
    };
    Some(
        t.iter()
            .filter_map(|(k, v)| Some((k.clone(), read_value(v)?)))
            .collect(),
    )
}

/// The channel out of rustup's legacy one-line file, where that is what this is.
///
/// The extensionless `rust-toolchain` predates the TOML form and holds a bare
/// channel name and nothing else, so it fails the parse. Treating that failure
/// as "declares nothing" is silent in the bad direction: a root pinned in the
/// legacy file with a floating copy under it would be excused, which is exactly
/// what this lint exists to refuse.
///
/// One non-empty, non-comment line and no `=` in it is the whole test. Anything
/// else is a malformed TOML file rather than a legacy one, and that is rustup's
/// complaint to make.
fn legacy_channel(text: &str) -> Option<String> {
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'));
    let one = lines.next()?;
    if lines.next().is_some() || one.contains('=') || one.starts_with('[') {
        return None;
    }
    Some(one.to_string())
}

/// One value, or nothing for a type this does not compare.
fn read_value(v: &toml::Value) -> Option<Value> {
    match v {
        toml::Value::String(s) => Some(Value::Str(s.clone())),
        toml::Value::Array(items) => {
            Some(Value::Arr(
                items
                    .iter()
                    .filter_map(|i| i.as_str().map(str::to_string))
                    .collect(),
            ))
        },
        _ => None,
    }
}

/// Whether the copy carries the marker that silences this.
fn allowed(text: &str) -> bool {
    text.lines()
        .any(|l| l.contains("lint:allow(the-mock-toolchain-matches-the-root)"))
}

/// What the copy fails to declare, one complaint per key, in the root's order.
///
/// Empty when the copy declares everything the root does. Public so a readiness
/// report can ask the same question of the same two files without a second copy
/// of the rule, and so the tests can plant text rather than a directory tree.
#[must_use]
pub fn disagreements(root: &str, copy: &str) -> Vec<String> {
    if allowed(copy) {
        return Vec::new();
    }
    // Either file failing to parse ends this, and the direction is the same on
    // both sides: an unreadable copy declares nothing that can be compared, and
    // an unreadable root demands nothing that can be. Complaining here would
    // name the wrong fault, since a copy that does declare `channel` would be
    // reported as not declaring it. rustup refuses a malformed toolchain file
    // on its own account and that is the complaint the reader needs.
    let (Some(mine), Some(theirs)) = (toolchain_table(copy), toolchain_table(root)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (key, want) in theirs {
        let Some((_, got)) = mine.iter().find(|(k, _)| *k == key) else {
            out.push(format!(
                "`{key}` is declared at the repository root and not here, so the root's value \
                 does not reach any command the gate runs"
            ));
            continue;
        };
        match (&want, got) {
            (Value::Str(w), Value::Str(g)) if w != g => {
                out.push(format!(
                    "`{key}` is `{g}` here and `{w}` at the repository root"
                ));
            },
            (Value::Arr(w), Value::Arr(g)) => {
                let missing: Vec<&String> = w.iter().filter(|i| !g.contains(i)).collect();
                if !missing.is_empty() {
                    let names: Vec<String> =
                        missing.into_iter().map(|m| format!("`{m}`")).collect();
                    out.push(format!(
                        "`{key}` here is missing {}, which the repository root declares",
                        names.join(", ")
                    ));
                }
            },
            (w, g) if std::mem::discriminant(w) != std::mem::discriminant(g) => {
                out.push(format!(
                    "`{key}` is a different kind of value here than at the repository root"
                ));
            },
            _ => {},
        }
    }
    out
}

/// The toolchain file in `dir` under either spelling, or nothing where neither
/// is there.
///
/// The extensionless `rust-toolchain` first, because that is the one rustup
/// uses where both are present, and this lint has to read what rustup reads or
/// it reports about a file nothing runs on. Measured on rustup 1.29.0 with the
/// two spellings naming two different nightlies: it warns that both exist, says
/// which it takes, and takes `rust-toolchain`. The order here is the opposite of
/// what it reads like it should be, and `rustup_prefers_the_extensionless_file`
/// is what holds it.
fn toolchain_file(dir: &Path) -> Option<(std::path::PathBuf, String)> {
    for name in ["rust-toolchain", "rust-toolchain.toml"] {
        let path = dir.join(name);
        if let Ok(text) = std::fs::read_to_string(&path) {
            return Some((path, text));
        }
    }
    None
}

impl RepoLint for TheMockToolchainMatchesTheRoot {
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError> {
        let Some((copy_path, copy)) = toolchain_file(ctx.mock_dir) else {
            return Vec::new();
        };
        let Some((_, root)) = toolchain_file(ctx.repo_root) else {
            return Vec::new();
        };
        // The offending path, not `"unknown"`. The siblings use that as a
        // fallback for when they cannot work out which file they are talking
        // about; this lint is about exactly one file and knows its name.
        let named = copy_path
            .strip_prefix(ctx.repo_root)
            .unwrap_or(&copy_path)
            .display()
            .to_string();
        disagreements(&root, &copy)
            .into_iter()
            .map(|d| {
                LintError::error(
                    named.clone(),
                    0,
                    "the-mock-toolchain-matches-the-root",
                    format!(
                        "the toolchain file under the mock directory shadows the repository \
                         root's for every command the gate runs, and {d}. Declare it here too, \
                         delete this file so the root's applies, or say why they differ with a \
                         `# lint:allow(the-mock-toolchain-matches-the-root)` line."
                    ),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "[toolchain]\nchannel = \"nightly-2026-05-28\"\ncomponents = [\"rustfmt\", \
                        \"clippy\", \"rust-src\"]\n";

    #[test]
    fn a_copy_declaring_the_same_thing_is_silent() {
        // The case the lint must not fire on, and the one every repository here
        // is meant to be in. Without it every arm below is equally consistent
        // with a lint that fires on any copy at all.
        assert_eq!(disagreements(ROOT, ROOT), Vec::<String>::new());
    }

    #[test]
    fn a_floating_channel_under_a_pinned_root_is_reported() {
        // The measured failure: three repositories carried exactly this and each
        // resolved a nightly four months newer than the one it declared.
        let copy = "[toolchain]\nchannel = \"nightly\"\ncomponents = [\"rustfmt\", \"clippy\", \
                    \"rust-src\"]\n";
        let d = disagreements(ROOT, copy);
        assert_eq!(d.len(), 1, "one key differs, so one complaint: {d:?}");
        assert!(d[0].contains("`channel` is `nightly` here"), "{}", d[0]);
        assert!(d[0].contains("nightly-2026-05-28"), "{}", d[0]);
    }

    #[test]
    fn a_copy_declaring_no_components_is_reported() {
        // The second half, and the quieter one: the formatter gate relies on the
        // pin supplying `rustfmt`, and a copy with no `components` key installs
        // whatever profile the machine defaults to.
        let copy = "[toolchain]\nchannel = \"nightly-2026-05-28\"\n";
        let d = disagreements(ROOT, copy);
        assert_eq!(d.len(), 1, "{d:?}");
        assert!(
            d[0].contains("`components` is declared at the repository root"),
            "{}",
            d[0]
        );
    }

    #[test]
    fn a_copy_dropping_one_component_is_reported_by_name() {
        let copy = "[toolchain]\nchannel = \"nightly-2026-05-28\"\ncomponents = [\"rustfmt\", \
                    \"clippy\"]\n";
        let d = disagreements(ROOT, copy);
        assert_eq!(d.len(), 1, "{d:?}");
        assert!(
            d[0].contains("`rust-src`"),
            "the missing one is named: {}",
            d[0]
        );
        assert!(
            !d[0].contains("`rustfmt`"),
            "the present ones are not: {}",
            d[0]
        );
    }

    #[test]
    fn a_copy_adding_a_component_is_silent() {
        // An array is a superset rather than an equality, because the mock
        // workspace wanting a component the root does not need is a real thing
        // and is not the failure this guards.
        let copy = "[toolchain]\nchannel = \"nightly-2026-05-28\"\ncomponents = [\"rustfmt\", \
                    \"clippy\", \"rust-src\", \"miri\"]\n";
        assert_eq!(disagreements(ROOT, copy), Vec::<String>::new());
    }

    #[test]
    fn a_key_only_the_copy_declares_is_silent() {
        let copy = "[toolchain]\nchannel = \"nightly-2026-05-28\"\ncomponents = [\"rustfmt\", \
                    \"clippy\", \"rust-src\"]\ntargets = [\"wasm32-unknown-unknown\"]\n";
        assert_eq!(disagreements(ROOT, copy), Vec::<String>::new());
    }

    #[test]
    fn the_marker_silences_a_copy_that_would_otherwise_be_reported() {
        let copy = "# lint:allow(the-mock-toolchain-matches-the-root) reason: the mock workspace \
                    tracks the newest nightly on purpose\n[toolchain]\nchannel = \"nightly\"\n";
        assert_eq!(disagreements(ROOT, copy), Vec::<String>::new());
    }

    #[test]
    fn the_marker_control_is_the_same_file_without_it() {
        // What stops the arm above from establishing that the lint is inert on
        // a channel-only copy for some reason other than the marker.
        let copy = "[toolchain]\nchannel = \"nightly\"\n";
        assert_eq!(
            disagreements(ROOT, copy).len(),
            2,
            "channel differs and components is absent"
        );
    }

    #[test]
    fn a_key_outside_the_toolchain_table_is_not_read() {
        // Only `[toolchain]` is read, so a `channel` under another table is a
        // different key and the two sides do not disagree over it.
        let root = "[toolchain]\nchannel = \"nightly-2026-05-28\"\n[other]\nchannel = \"beta\"\n";
        let copy = "[toolchain]\nchannel = \"nightly-2026-05-28\"\n[other]\nchannel = \"stable\"\n";
        assert_eq!(disagreements(root, copy), Vec::<String>::new());
    }

    #[test]
    fn a_trailing_comment_is_not_part_of_the_value() {
        let copy = "[toolchain]\nchannel = \"nightly-2026-05-28\"  # the stack's pin\ncomponents \
                    = [\"rustfmt\", \"clippy\", \"rust-src\"]\n";
        assert_eq!(disagreements(ROOT, copy), Vec::<String>::new());
    }

    /// A repository with whichever of the two files the arms want, and the lint
    /// run over it as the gate runs it.
    ///
    /// The unit above tests the rule; this tests the wiring, which is the half
    /// that can be wrong in a way no amount of rule testing sees: reading the
    /// two files off the wrong ends of the context reports a repository as
    /// agreeing with itself and passes every arm above.
    fn run(root: Option<&str>, copy: Option<&str>) -> Vec<LintError> {
        run_named(
            &[("rust-toolchain.toml", root)],
            &[("rust-toolchain.toml", copy)],
        )
    }

    /// The same wiring with the filenames given, because the spelling is the
    /// thing under test in the arms below and `run` fixes it.
    ///
    /// Each end takes a list, so a directory can hold both spellings at once,
    /// which is the case rustup warns about and the one the loop's order
    /// decides.
    fn run_named(root: &[(&str, Option<&str>)], copy: &[(&str, Option<&str>)]) -> Vec<LintError> {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        let mock = repo.join("mock");
        std::fs::create_dir_all(&mock).unwrap();
        for (name, body) in root {
            if let Some(t) = body {
                std::fs::write(repo.join(name), t).unwrap();
            }
        }
        for (name, body) in copy {
            if let Some(t) = body {
                std::fs::write(mock.join(name), t).unwrap();
            }
        }
        let registry = crate::RegistryView::default();
        let no_crates = std::collections::BTreeSet::new();
        let ctx = RepoContext {
            mock_dir:    &mock,
            repo_root:   repo,
            all_crates:  &no_crates,
            src_dirs:    &[],
            invocation:  None,
            canon_paths: &[],
            open_panels: &[],
            registry:    &registry,
        };
        TheMockToolchainMatchesTheRoot.check_repo(&ctx)
    }

    #[test]
    fn the_lint_reads_the_two_files_off_the_right_ends_of_the_context() {
        let copy = "[toolchain]\nchannel = \"nightly\"\ncomponents = [\"rustfmt\", \"clippy\", \
                    \"rust-src\"]\n";
        let errs = run(Some(ROOT), Some(copy));
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(
            errs[0].message.contains("`channel` is `nightly` here"),
            "{}",
            errs[0].message
        );
    }

    #[test]
    fn a_repository_with_no_copy_is_silent() {
        // The ordinary case and the one the rule prefers: one file, nothing to
        // keep in step. arvo is in it, which is why arvo resolves its root pin.
        assert!(run(Some(ROOT), None).is_empty());
    }

    #[test]
    fn a_repository_with_no_root_file_is_silent() {
        // Nothing to judge against. Reporting here would be asserting the copy
        // is wrong against a pin the project never made.
        let copy = "[toolchain]\nchannel = \"nightly\"\n";
        assert!(run(None, Some(copy)).is_empty());
    }

    // The four shapes the hand scanner got wrong. Three of them produced a
    // false complaint, which is noise. The second is the one that matters: it
    // was silent, and silent in the direction the lint exists to catch.

    #[test]
    fn a_multi_line_array_in_the_copy_is_read() {
        let copy = "[toolchain]\nchannel = \"nightly-2026-05-28\"\ncomponents = [\n  \
                    \"rustfmt\",\n  \"clippy\",\n  \"rust-src\",\n]\n";
        assert_eq!(
            disagreements(ROOT, copy),
            Vec::<String>::new(),
            "the same components, written over several lines"
        );
    }

    #[test]
    fn a_multi_line_array_in_the_root_is_still_demanded() {
        // The silent one. Dropping the root's key means nothing is compared, so
        // a copy declaring none of its components passes and the lint reports a
        // repository as agreeing with itself.
        let root = "[toolchain]\nchannel = \"nightly-2026-05-28\"\ncomponents = [\n  \
                    \"rustfmt\",\n  \"clippy\",\n  \"rust-src\",\n]\n";
        let copy = "[toolchain]\nchannel = \"nightly-2026-05-28\"\n";
        let d = disagreements(root, copy);
        assert_eq!(
            d.len(),
            1,
            "the root declares components and the copy does not: {d:?}"
        );
        assert!(d[0].contains("`components`"), "{}", d[0]);
    }

    #[test]
    fn literal_strings_are_the_same_values_as_basic_ones() {
        let copy = "[toolchain]\nchannel = 'nightly-2026-05-28'\ncomponents = ['rustfmt', \
                    'clippy', 'rust-src']\n";
        assert_eq!(disagreements(ROOT, copy), Vec::<String>::new());
    }

    #[test]
    fn a_spaced_table_header_is_the_same_table() {
        let copy = "[ toolchain ]\nchannel = \"nightly-2026-05-28\"\ncomponents = [\"rustfmt\", \
                    \"clippy\", \"rust-src\"]\n";
        assert_eq!(disagreements(ROOT, copy), Vec::<String>::new());
    }

    #[test]
    fn a_legacy_root_still_demands_its_channel() {
        // The silent one, and the reason `toolchain_file` may name the
        // extensionless spelling at all. rustup's older form is a bare channel
        // line, which is not TOML; reading the parse failure as "declares
        // nothing" excuses a floating copy under a pinned root, which is what
        // this lint exists to refuse.
        let d = disagreements(
            "nightly-2026-05-28\n",
            "[toolchain]\nchannel = \"nightly\"\n",
        );
        assert_eq!(d.len(), 1, "{d:?}");
        assert!(d[0].contains("`channel` is `nightly` here"), "{}", d[0]);
    }

    #[test]
    fn a_legacy_copy_is_read_as_the_channel_it_names() {
        let d = disagreements(ROOT, "nightly-2026-05-28\n");
        assert_eq!(
            d.len(),
            1,
            "the channel agrees, the components are absent: {d:?}"
        );
        assert!(d[0].contains("`components`"), "{}", d[0]);
    }

    #[test]
    fn a_legacy_file_with_a_comment_above_it_is_still_one_line() {
        let d = disagreements(
            "# the stack's pin\nnightly-2026-05-28\n",
            "nightly-2026-05-28\n",
        );
        assert_eq!(d, Vec::<String>::new());
    }

    #[test]
    fn two_bare_lines_are_not_a_legacy_file() {
        // The control for the three above. Without it they are equally
        // consistent with reading the first line of any unparseable file as a
        // channel, which would invent a pin out of a malformed TOML file.
        assert_eq!(
            disagreements("nightly-2026-05-28\nsomething else\n", ROOT),
            Vec::<String>::new()
        );
    }

    #[test]
    fn an_unparseable_file_claims_nothing_and_demands_nothing() {
        // Both directions, because the safe answer differs per side and this is
        // the one place the parse can fail. rustup complains about a malformed
        // toolchain file already; saying it again here reports one fault twice.
        let broken = "[toolchain\nchannel = \"nightly\"\n";
        assert_eq!(
            disagreements(ROOT, broken),
            Vec::<String>::new(),
            "unreadable copy"
        );
        assert_eq!(
            disagreements(broken, ROOT),
            Vec::<String>::new(),
            "unreadable root"
        );
    }

    #[test]
    fn a_value_of_a_type_this_does_not_compare_is_dropped() {
        // `profile` is a string and `components` an array; anything else is not
        // in the schema. A file carrying one is not reported as disagreeing
        // with itself over a key nothing here knows how to compare.
        let root = "[toolchain]\nchannel = \"nightly-2026-05-28\"\ncomponents = [\"rustfmt\", \
                    \"clippy\", \"rust-src\"]\nsomething = 3\n";
        assert_eq!(disagreements(root, ROOT), Vec::<String>::new());
    }

    /// The extensionless spelling is the whole subject of this round, and until
    /// this arm existed nothing read it off a disk.
    ///
    /// Every other arm hands `disagreements` two strings, and the one wiring
    /// arm wrote `rust-toolchain.toml` at both ends, so `toolchain_file`'s loop
    /// was never taken past its first entry. Drop the other entry and all
    /// fourteen of them stay green while the defect returns.
    #[test]
    fn the_extensionless_spelling_is_read_off_the_disk() {
        let copy = "nightly\n";
        let errs = run_named(
            &[("rust-toolchain", Some(ROOT))],
            &[("rust-toolchain", Some(copy))],
        );
        // Two, because a legacy file carries a channel and can carry nothing
        // else, so the root's `components` is missing from it as well.
        assert_eq!(errs.len(), 2, "{errs:?}");
        assert!(
            errs[0].message.contains("`channel` is `nightly` here"),
            "{}",
            errs[0].message
        );
    }

    /// Where a directory holds both, rustup takes the extensionless one, so
    /// this lint has to as well or it reports about a file nothing runs on.
    ///
    /// Measured on rustup 1.29.0: with `rust-toolchain.toml` naming
    /// `nightly-2026-06-18` and `rust-toolchain` naming `nightly-2026-05-28`,
    /// `rustup show active-toolchain` warns that both exist and resolves
    /// `nightly-2026-05-28`, naming `rust-toolchain` as the override.
    ///
    /// The control is the pairing: the `.toml` in the copy agrees with the root
    /// and the extensionless one does not, so an implementation reading the
    /// `.toml` returns no error and this arm fails. Reversing the loop's order
    /// is what makes it pass.
    #[test]
    fn rustup_prefers_the_extensionless_file() {
        let errs = run_named(
            &[("rust-toolchain.toml", Some(ROOT))],
            &[
                ("rust-toolchain.toml", Some(ROOT)),
                ("rust-toolchain", Some("nightly\n")),
            ],
        );
        assert_eq!(
            errs.len(),
            2,
            "the copy rustup would actually use declares a floating channel and no \
             components: {errs:?}"
        );
        assert!(
            errs[0].message.contains("`channel` is `nightly` here"),
            "{}",
            errs[0].message
        );
    }

    #[test]
    fn a_hash_inside_a_quoted_value_stays_in_it() {
        // Nothing in this schema is likely to carry one, and the line scan this
        // replaced cut the value there and dropped half of it silently.
        let table = toolchain_table("[toolchain]\nchannel = \"a#b\"\n");
        assert_eq!(
            table,
            Some(vec![("channel".to_string(), Value::Str("a#b".to_string()))])
        );
    }
}
