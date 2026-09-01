//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `mock test`: run the tests of every tree mockspace owns.
//!
//! **The problem is wider than benches and was solved only for benches.**
//! Everything mockspace compiles that is not a member of the consumer's mock
//! workspace is invisible to `cargo test` run there, and mockspace compiles
//! three such trees: the bench crates, the tool crates, and the generated lint
//! cdylib. A `cargo test` in `mock/` reaches the members and nothing else, so a
//! repository whose `members` list is empty runs no tests at all while
//! appearing to.
//!
//! `mock bench test` closed one third of that. The consequence of leaving the
//! other two open is not abstract: a consumer repository with six tools and
//! four lints had nowhere to put a test for any of them, and grew a directory
//! of loose scripts in another language instead, outside the workspace, run by
//! whoever remembered it existed.
//!
//! So this runs all four, reports them separately, and fails if any fails.
//!
//! **The lint tree is the one that needed no new mechanism.** The generated
//! cdylib already `#[path]`-includes every `mock/lints/*.rs`, so a
//! `#[cfg(test)] mod tests` in a lint file compiles into it and `cargo test`
//! in the generated directory runs it. What was missing was anything that ran
//! that command.
//!
//! **What made the trees reachable also made them expensive.** The empty
//! `[workspace]` table each tool crate carries is what keeps a cdylib out of
//! the consumer's dependency graph, and it is equally what makes the crate its
//! own cargo root: its `target/` sits beside its own manifest and is shared
//! with nothing. So every tool in a repository compiles that repository's
//! dependency set again and keeps its own copy, on every run, and the lint
//! crate taking those tools as path dependencies pays for one more. One shared
//! `--target-dir` for the roots collapses that, and `mock bench test` had
//! already done it for the bench arms.
//!
//! The counts differ per repository and move whenever a tool is added, so they
//! are not written here. `mock tools` reports what a given tree holds.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::config::Config;

/// One tree, its label, and where its manifest lives.
struct Tree {
    what:     &'static str,
    dir:      PathBuf,
    /// Why this tree is not reached by a plain `cargo test` in `mock/`, so a
    /// reader of the output knows what each row is for.
    because:  &'static str,
    /// Whether this tree's own manifest declares `[workspace]`, which is what
    /// makes it a cargo root and the reason it needs [`shared_target_dir`]:
    /// its default `target/` sits next to that manifest and is shared with
    /// nothing.
    ///
    /// Not the same property as being outside a plain `cargo test`. A crate
    /// under `tools/` that the workspace lists in `members` is also outside
    /// nothing, and is not a root either: its default is already `mock/target/`
    /// and redirecting it opens a second build tree rather than closing one.
    own_root: bool,
}

pub fn run(cfg: &Config, args: &[&str]) -> ExitCode {
    let mut trees: Vec<Tree> = Vec::new();

    // The workspace itself, but only when it has a member.
    //
    // A virtual manifest with an empty `members` does not run zero tests, it
    // ERRORS: "the manifest is virtual, and the workspace has no members". So a
    // repository that keeps its crates elsewhere gets a hard failure from the
    // one tree a plain `cargo test` was supposed to cover, and any caller that
    // reports the result honestly reports a failure caused by nothing being
    // there. Skipped with a note instead.
    let ws = cfg.mock_dir.join("Cargo.toml");
    if ws.exists() {
        // Asked of `cargo_gate`, which parses the manifest with `toml_edit`
        // rather than scanning it for substrings, and which the readiness
        // report already consumes. A second implementation of this question was
        // wrong on two real manifests: a `[package]` beside an empty `members`
        // reported as memberless when `cargo test` there passes, and a
        // commented-out `members` line read as live.
        //
        // The first version of this file justified the hand-rolled scan as
        // needing no dependency. `toml_edit` is a direct dependency of this
        // crate, so that was not true either.
        if crate::entry::cargo_gate::is_memberless_virtual_workspace(&cfg.mock_dir) {
            println!(
                "note: {} declares no workspace member, so there is nothing for a plain\n      \
                 cargo test to reach. The trees below are the ones that matter here.",
                ws.display()
            );
        } else {
            trees.push(Tree {
                what:     "workspace members",
                dir:      cfg.mock_dir.clone(),
                because:  "reached by a plain cargo test",
                own_root: false,
            });
        }
    }

    // Tool and bench crates sit INSIDE the workspace directory without being
    // members, which cargo refuses outright: "current package believes it's in
    // a workspace when it's not". The generated lint crate already solves this
    // for itself with a leading empty `[workspace]` table, making it its own
    // root, and a tool crate needs the same one line.
    //
    // Reported here as the one-line fix rather than passed through as cargo's
    // message, which suggests adding the crate to `members` and that is the
    // wrong direction: membership would put a cdylib into the consumer's
    // dependency graph.
    let mut orphaned: Vec<PathBuf> = Vec::new();
    for dir in crate_dirs(&cfg.mock_dir.join("tools")) {
        if is_orphaned(&dir, &cfg.mock_dir) {
            orphaned.push(dir);
            continue;
        }
        // Two ways past `is_orphaned` and only one of them is a root. A
        // manifest declaring `[workspace]` is its own; a crate the workspace
        // lists in `members` is not, and already builds into `mock/target/`.
        let own_root = declares_its_own_workspace(&dir);
        trees.push(Tree {
            what: "tool",
            dir,
            because: if own_root {
                "compiled as a path dependency of the lint cdylib, never a member"
            } else {
                "listed in the workspace members, so a plain cargo test reaches it too"
            },
            own_root,
        });
    }

    // Benches are NOT walked here. `mock bench test` already runs them, and it
    // does the one thing a directory walk cannot: a freshly `mock bench init`ed
    // tree has no manifest anywhere, because the arm manifests are generated on
    // demand under `target/`. So the walk finds nothing on the canonical layout
    // and reports it as nothing to run, which is the exact failure `bench.rs`
    // records having already fixed once.
    let bench_dir = cfg.mock_dir.join("benches");
    let has_benches = bench_dir.exists()
        && std::fs::read_dir(&bench_dir)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false);

    // The generated lint crate, which exists only after something generates it.
    // Not generated here: generating is the lint path's job and doing it from
    // the test path would mean two places that know how.
    let lints_gen = crate::build_dir::ensure_under_target(&cfg.mock_dir, &["mockspace-lints"]);
    if lints_gen.join("Cargo.toml").exists() {
        trees.push(Tree {
            what:     "lints",
            dir:      lints_gen,
            because:  "generated under target/, its own workspace, never a member",
            own_root: true,
        });
    } else {
        eprintln!(
            "note: the lint crate has not been generated, so `mock/lints/*.rs` tests are \
             not in this run. `mock check` generates it."
        );
    }

    if !orphaned.is_empty() {
        eprintln!(
            "\nmock test: {} crate(s) cannot be tested where they sit. Each is inside the\n\
             mock workspace directory and is not a member, which cargo refuses. Add an\n\
             empty `[workspace]` table at the top of each manifest, the way the generated\n\
             lint crate does, and it becomes its own root:\n",
            orphaned.len()
        );
        for d in &orphaned {
            eprintln!("    {}/Cargo.toml", d.display());
        }
        eprintln!();
    }

    if trees.is_empty() && !has_benches {
        eprintln!(
            "mock test: no tree to test under {}",
            cfg.mock_dir.display()
        );
        return ExitCode::FAILURE;
    }

    let mut failed = Vec::new();
    // Counted rather than derived from `trees`, because the benches tree is
    // run below and is deliberately not in it. Reporting `trees.len()` said
    // `7 tree(s) green` on a run that tested eight, and `1 of 0` where the
    // only tree was benches.
    let ran = ran_count(trees.len(), has_benches);

    if has_benches {
        println!("\n=== benches : {} ===", bench_dir.display());
        println!("    (compiled per arm with generated manifests, never a member)");
        if crate::bench::cmd(cfg, &["test"]) != ExitCode::SUCCESS {
            failed.push(format!("benches at {}", bench_dir.display()));
        }
    }

    let shared = shared_target_dir(cfg, &trees, args);

    for t in &trees {
        println!("\n=== {} : {} ===", t.what, t.dir.display());
        println!("    ({})", t.because);
        let mut cmd = crate::entry::cargo_gate::cargo(&t.dir, &["test"]);
        if let Some(dir) = redirect_for(t, shared.as_ref()) {
            // Before `args`, so a `--` the caller passed still separates
            // cargo's flags from the test binary's.
            cmd.arg("--target-dir").arg(dir);
        }
        let ok = cmd
            .args(args)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            failed.push(format!("{} at {}", t.what, t.dir.display()));
        }
    }

    println!();
    if failed.is_empty() {
        println!("mock test: {ran} tree(s) green");
        ExitCode::SUCCESS
    } else {
        for f in &failed {
            eprintln!("mock test: FAILED {f}");
        }
        eprintln!("mock test: {} of {ran} tree(s) failed", failed.len());
        ExitCode::FAILURE
    }
}

/// How many trees this run actually tested.
///
/// The benches tree is run beside `trees` rather than inside it, for the
/// reason stated where the walk skips it, so the population the report counts
/// is not the population the loop iterates.
fn ran_count(trees: usize, has_benches: bool) -> usize {
    trees + usize::from(has_benches)
}

/// One target directory for every tree here that is its own cargo root.
///
/// The empty `[workspace]` table that keeps a tool crate out of the consumer's
/// dependency graph also makes it its own root, so cargo builds it into a
/// `target/` beside its own manifest and shares nothing with its neighbours.
/// Six tool crates in one repository compile thirteen dependency crates each,
/// `tree-sitter` and the `regex` stack among them, with only the leaf
/// differing. The mechanism and its controls are in
/// `mock/research/202608151554_probes/standalone-target-dir-sharing/`.
///
/// The lint crate joins them rather than getting a directory of its own,
/// because it takes the tools as path dependencies and so wants the same
/// compiled copies.
///
/// The members tree is not redirected, which is what `own_root` on [`Tree`]
/// decides. Its default is `mock/target/`, which a plain `cargo test` in
/// `mock/` and every other cargo invocation this engine makes already use, so
/// pointing it elsewhere would stop it sharing rather than start it.
///
/// `mock bench test` reached the same shape first, for the same reason on a
/// larger tree; see the target directory it passes in `bench.rs`.
fn shared_target_dir(cfg: &Config, trees: &[Tree], args: &[&str]) -> Option<PathBuf> {
    // Nothing to share means nothing to create. A repository with members and
    // no tools and no generated lint crate would otherwise get an empty
    // `mock/target/mockspace-test/` it never writes into, which is the split
    // `build_dir.rs` exists to keep clean.
    let wanted = trees.iter().any(|t| t.own_root);
    let inherited = inherited_target_dir();
    (wanted && may_share(args, inherited.as_deref()))
        .then(|| crate::build_dir::ensure_under_target(&cfg.mock_dir, &["mockspace-test"]))
}

/// The `--target-dir` this tree gets, if any.
///
/// One line, and it is a function so a test can assert the per-tree half of the
/// decision. Eight arms covered [`may_share`] and none covered this, which is
/// why a workspace member under `tools/` was redirected into a second build
/// tree with every test still green.
fn redirect_for<'a>(t: &Tree, shared: Option<&'a PathBuf>) -> Option<&'a PathBuf> {
    t.own_root.then_some(shared).flatten()
}

/// Every environment spelling that already fixes where cargo builds.
///
/// Both are honoured on the pinned toolchain and either one is enough to
/// decline. `.cargo/config.toml`'s `[build] target-dir` is a third mechanism
/// and is NOT read here: it is resolved by walking up from each tool's own
/// directory, so there is no single answer to read, and the cost of missing it
/// is the duplication that was there before rather than a wrong build.
fn inherited_target_dir() -> Option<std::ffi::OsString> {
    // `CARGO_TARGET_DIR` first, which is cargo's own precedence over the
    // config-backed spelling. Only presence is read, so the order changes
    // nothing here and is kept honest for the next reader.
    std::env::var_os("CARGO_TARGET_DIR").or_else(|| std::env::var_os("CARGO_BUILD_TARGET_DIR"))
}

/// Whether this run may pass a `--target-dir` of its own.
///
/// Split out from [`shared_target_dir`] with the environment passed in rather
/// than read, so the decision is testable without a test mutating the process
/// environment out from under every other arm running beside it.
fn may_share(args: &[&str], inherited: Option<&std::ffi::OsStr>) -> bool {
    // An inherited target directory already puts every build in one place, and
    // `--target-dir` overrides it, so passing ours would open a second build
    // directory beside the one the machine chose and duplicate the exact thing
    // this is here to stop.
    if inherited.is_some() {
        return false;
    }
    // The caller's own flag wins, and only up to `--`, after which the tokens
    // belong to the test binary and say nothing about where cargo builds.
    !args
        .iter()
        .take_while(|a| **a != "--")
        .any(|a| *a == "--target-dir" || a.starts_with("--target-dir="))
}

/// Every directory under `root` holding a `Cargo.toml`, one level down.
///
/// One level rather than a walk, because both `tools/` and `benches/` are flat
/// by convention and a walk would descend into each crate's own `target/` and
/// find the manifests cargo writes there.
fn crate_dirs(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("Cargo.toml").exists())
        .collect();
    out.sort();
    out
}

/// Whether this crate's own manifest declares `[workspace]`, making it a cargo
/// root whose `target/` sits beside it and is shared with nothing.
///
/// The one question behind two decisions: whether cargo will refuse the crate
/// where it sits, and whether it needs a shared target directory. Reading it
/// once means the two cannot disagree, which is how a workspace member under
/// `tools/` came to be redirected into a second build tree.
fn declares_its_own_workspace(crate_dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(crate_dir.join("Cargo.toml")) else {
        return false;
    };
    // Parsed rather than searched. A substring match reads a commented-out
    // table, or the word inside a description string, as a root, and gets the
    // answer backwards in the direction that matters: the crate is then given a
    // shared target directory it must not have, or denied one it should.
    // A manifest that does not parse is not a root, which is the same answer an
    // absent one gives and is what cargo will say about it too.
    //
    // `[workspace.package]` and its siblings count. They define the `workspace`
    // key just as the bare table does, so cargo reads the manifest as a root
    // either way, and only the bare-table spelling was being looked for.
    //
    // A different question from `cargo_gate::is_memberless_virtual_workspace`,
    // which asks whether a root has anything to build. Both read the same file
    // and neither answers the other, so they stay separate.
    text.parse::<toml_edit::DocumentMut>()
        .is_ok_and(|doc| doc.get("workspace").is_some())
}

/// Whether a crate sits inside a workspace directory without being a member and
/// without declaring itself a root.
fn is_orphaned(crate_dir: &Path, mock_dir: &Path) -> bool {
    if !crate_dir.starts_with(mock_dir) {
        return false;
    }
    if declares_its_own_workspace(crate_dir) {
        return false;
    }
    let ws = std::fs::read_to_string(mock_dir.join("Cargo.toml")).unwrap_or_default();
    let Ok(rel) = crate_dir.strip_prefix(mock_dir) else {
        return false;
    };
    !ws.contains(&format!("\"{}\"", rel.display()))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    /// The ordinary case, and the one the whole change is for.
    #[test]
    fn nothing_in_the_way_means_the_roots_share_one_directory() {
        assert!(may_share(&[], None));
        assert!(may_share(&["--nocapture"], None));
    }

    /// Deleting this check leaves a machine that already shares one build
    /// directory with two of them, which is the duplication this exists to
    /// remove, arrived at from the other side.
    #[test]
    fn an_inherited_target_dir_is_left_alone() {
        assert!(!may_share(&[], Some(OsStr::new("/somewhere/shared"))));
        assert!(!may_share(&["--nocapture"], Some(OsStr::new("/elsewhere"))));
    }

    /// Set but empty is read as set. Cargo's own handling of an empty value is
    /// not something this can observe, and declining to pass a flag is the
    /// reading that cannot break anything: the worst it costs is the
    /// duplication that was there before.
    #[test]
    fn an_empty_inherited_value_still_counts_as_set() {
        assert!(!may_share(&[], Some(OsStr::new(""))));
    }

    #[test]
    fn the_callers_own_flag_wins_in_both_spellings() {
        assert!(!may_share(&["--target-dir", "/mine"], None));
        assert!(!may_share(&["--target-dir=/mine"], None));
        assert!(!may_share(&["--release", "--target-dir", "/mine"], None));
    }

    /// After `--` the tokens are the test binary's and say nothing about where
    /// cargo builds, so one there must not suppress the shared directory.
    /// Without the `take_while` this arm is the one that fails.
    #[test]
    fn the_same_flag_past_the_separator_is_not_cargos() {
        assert!(may_share(&["--", "--target-dir"], None));
        assert!(may_share(&["--", "--target-dir=/not-cargos"], None));
        assert!(may_share(&["--nocapture", "--", "--target-dir"], None));
    }

    /// One before and one after: the one before is cargo's and decides it.
    #[test]
    fn a_flag_on_each_side_of_the_separator_is_decided_by_the_first() {
        assert!(!may_share(
            &["--target-dir", "/mine", "--", "--target-dir"],
            None
        ));
    }

    /// `--target` selects a triple and is not this flag. A prefix test written
    /// without the `=` would swallow it, and cross-compiling would silently
    /// lose the shared directory.
    #[test]
    fn a_neighbouring_flag_is_not_mistaken_for_it() {
        assert!(may_share(&["--target", "x86_64-unknown-linux-gnu"], None));
        assert!(may_share(&["--target-dirty-is-not-a-flag"], None));
    }

    #[test]
    fn a_bare_separator_alone_changes_nothing() {
        assert!(may_share(&["--"], None));
    }

    /// Cargo honours this spelling too, and reading only the first one leaves
    /// a machine that has already chosen a build directory getting a second.
    #[test]
    fn the_build_spelling_of_the_variable_counts_as_set() {
        // The value `inherited_target_dir` would have found, either way round.
        assert!(!may_share(
            &[],
            Some(OsStr::new("/set/via/cargo_build_target_dir"))
        ));
    }

    /// The report counts what ran, and the benches tree runs outside `trees`.
    /// Before this, seven trees plus benches printed `7 tree(s) green`, and a
    /// benches-only repository printed `1 of 0 tree(s) failed`.
    #[test]
    fn the_report_counts_the_benches_tree_it_also_ran() {
        assert_eq!(ran_count(7, true), 8);
        assert_eq!(ran_count(7, false), 7);
        assert_eq!(ran_count(0, true), 1);
        assert_eq!(ran_count(0, false), 0);
    }

    fn tree(what: &'static str, own_root: bool) -> Tree {
        Tree {
            what,
            dir: PathBuf::from("/nowhere"),
            because: "fixture",
            own_root,
        }
    }

    /// The property the whole change is for, which nothing asserted until a
    /// reviewer found a member tool being redirected into a second build tree.
    #[test]
    fn only_a_tree_that_is_its_own_root_is_redirected() {
        let shared = PathBuf::from("/shared");
        assert_eq!(
            redirect_for(&tree("tool", true), Some(&shared)),
            Some(&shared)
        );
        assert_eq!(
            redirect_for(&tree("lints", true), Some(&shared)),
            Some(&shared)
        );
        assert_eq!(
            redirect_for(&tree("workspace members", false), Some(&shared)),
            None
        );
        // A tool the workspace lists in `members` reaches this with
        // `own_root == false`, and its default is already `mock/target/`.
        assert_eq!(redirect_for(&tree("tool", false), Some(&shared)), None);
    }

    /// Deleting the `may_share` guard must not leak a directory in through the
    /// per-tree half, so the two halves are asserted independently.
    #[test]
    fn no_shared_directory_means_no_redirect_for_anyone() {
        assert_eq!(redirect_for(&tree("tool", true), None), None);
        assert_eq!(redirect_for(&tree("workspace members", false), None), None);
    }

    /// `[workspace]` in the crate's own manifest is the whole test, and a
    /// crate without one is not a root however it got past `is_orphaned`.
    #[test]
    fn a_root_is_the_manifest_declaring_a_workspace_and_nothing_else() {
        let tmp = std::env::temp_dir().join(format!(
            "mockspace-test-own-root-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let root = tmp.join("is-a-root");
        let member = tmp.join("is-a-member");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&member).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\n\n[package]\nname = \"r\"\n",
        )
        .unwrap();
        std::fs::write(member.join("Cargo.toml"), "[package]\nname = \"m\"\n").unwrap();

        assert!(declares_its_own_workspace(&root));
        assert!(!declares_its_own_workspace(&member));
        // A directory with no manifest at all reads as not a root rather than
        // panicking, which is what `is_orphaned` has always relied on.
        assert!(!declares_its_own_workspace(&tmp.join("nothing-here")));

        // The four a substring match gets wrong. The first two are the ones
        // that bite: a manifest naming the table in a comment or in prose was
        // handed a shared target directory it must not have, and nothing said
        // so, because the answer looks the same either way from outside.
        let cases: [(&str, bool, &str); 4] = [
            (
                "# [workspace] was removed when this became a member\n[package]\nname = \"c\"\n",
                false,
                "a commented-out table is not a declaration",
            ),
            (
                "[package]\nname = \"d\"\ndescription = \"how [workspace] roots behave\"\n",
                false,
                "the word inside a string is not a declaration",
            ),
            (
                "[workspace.package]\nversion = \"0.1.0\"\n\n[package]\nname = \"e\"\n",
                true,
                "a dotted workspace table defines the key just as the bare one does, \
                 and cargo reads the manifest as a root either way",
            ),
            (
                "[package\nname = \"f\"\n",
                false,
                "a manifest that does not parse is not a root, which is the answer \
                 cargo gives it too",
            ),
        ];
        for (i, (text, want, why)) in cases.iter().enumerate() {
            let dir = tmp.join(format!("case-{i}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("Cargo.toml"), text).unwrap();
            assert_eq!(
                declares_its_own_workspace(&dir),
                *want,
                "{why}; manifest was {text:?}"
            );
        }

        std::fs::remove_dir_all(&tmp).ok();
    }
}
