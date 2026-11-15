//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A bench runs only on the driver `mock bench run` generates.
//!
//! The generated driver is the one runner, and it writes every run into the
//! one layout. A runner of a consumer's own took the run over and wrote its
//! results wherever it chose: at the bench root, in `.bench_history/`, under
//! names that carried no machine, so a re-run on another machine replaced the
//! first machine's numbers in place. Reconciling every such runner into the
//! layout after the fact is not something a harness can do without fault, so
//! the runners themselves are refused.
//!
//! What counts as a runner, under `<mock>/benches/`:
//!
//! - a `Cargo.toml` at the bench root, which used to take the whole run away
//!   from the generated driver;
//! - a `src/main.rs` at the bench root;
//! - any crate in the tree with a binary, a cargo bench or an example of its
//!   own: a `[[bin]]`, `[[bench]]` or `[[example]]` table, a `src/main.rs`, a
//!   `src/bin/` directory, or a `benches/` or `examples/` directory beside its
//!   manifest where cargo would discover it;
//! - a script anywhere in the tree: any file that opens with a shebang (`#!`,
//!   not an inner attribute's `#![`), whatever its extension, a nutshell script
//!   among them, and any `.sh`, `.bash`, `.zsh`, `.py` or `.nu` file;
//! - a `Makefile` or a `justfile`;
//! - a `.cargo/config.toml` (or `.cargo/config`) declaring `[alias]`es or a
//!   `runner` under a `[target.*]` table.
//!
//! What a runner did belongs in the hooks library (`src/lib.rs` at the bench
//! root, `pub fn hooks()`) and in the `bench.toml` files.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::{Entry, Lint, LintError, RepoContext, RepoLint, Severity, View};

const NAME: &str = "a-bench-runs-only-on-the-generated-driver";

/// One runner found in a bench tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Runner {
    /// The file that makes it a runner, relative to the bench tree root.
    pub path: PathBuf,
    /// What about it does.
    pub why:  &'static str,
}

impl fmt::Display for Runner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.path.display(), self.why)
    }
}

/// Every runner in the bench tree at `bench_dir`, read through `view`. Empty
/// where the tree does not exist. `mock bench run` and the lint ask this one
/// predicate, so the tool refuses exactly what the gate refuses.
#[must_use]
pub fn bench_runners(view: &View<'_>, bench_dir: &Path) -> Vec<Runner> {
    if !view.is_dir(bench_dir) {
        return Vec::new();
    }
    // Dot directories hold nothing a run reads, except `.cargo/`, whose
    // config can declare aliases that run a bench by another name.
    let files = view.walk(bench_dir, |_, e| {
        Entry::is_dir(e) && (e.name() == "target" || (e.name().starts_with('.') && e.name() != ".cargo"))
    });
    let mut found = Vec::new();
    for file in files {
        let Ok(rel) = file.strip_prefix(bench_dir) else {
            continue;
        };
        let name = rel.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let at_root = rel.parent().is_some_and(|p| p.as_os_str().is_empty());
        let mut say = |why| {
            found.push(Runner {
                path: rel.to_path_buf(),
                why,
            })
        };
        if name == "Cargo.toml" {
            if at_root {
                say("is a driver crate at the bench root, which takes the run away from the generated driver");
                continue;
            }
            let dir = file.parent().unwrap_or(bench_dir);
            if let Some(why) = own_binary(view, dir, &file) {
                say(why);
            }
        } else if rel == Path::new("src/main.rs") {
            say("is a `main` of the bench tree's own");
        } else if is_cargo_config(rel) {
            if let Some(why) = cargo_config_runs(view, &file) {
                say(why);
            }
        } else if RUNNER_FILES.contains(&name) {
            say("is a make or just file, a way to run the benches outside the harness");
        } else if SCRIPT_EXTENSIONS.iter().any(|ext| name.ends_with(ext)) {
            say("is a script beside the benches, a way to run them outside the harness");
        } else if view.bytes(&file).is_some_and(|b| b.starts_with(b"#!") && !b.starts_with(b"#![")) {
            say("is a script with a shebang beside the benches, a way to run them outside the harness");
        }
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found
}

/// File names that run things by name: make and just.
const RUNNER_FILES: [&str; 6] = ["Makefile", "makefile", "GNUmakefile", "justfile", "Justfile", ".justfile"];

/// Script extensions, every one of them a way to run a bench outside the harness.
const SCRIPT_EXTENSIONS: [&str; 5] = [".sh", ".bash", ".zsh", ".py", ".nu"];

/// Whether `rel` is a cargo configuration file, `.cargo/config.toml` or the
/// older `.cargo/config`.
fn is_cargo_config(rel: &Path) -> bool {
    let name = rel.file_name().and_then(|n| n.to_str()).unwrap_or("");
    rel.parent().and_then(|p| p.file_name()).is_some_and(|d| d == ".cargo")
        && (name == "config.toml" || name == "config")
}

/// How a cargo configuration file runs something by its own means: an
/// `[alias]`, or a `runner` under any `[target.<triple>]` or `[target.<cfg>]`
/// table, which wraps every binary cargo runs.
fn cargo_config_runs(view: &View<'_>, file: &Path) -> Option<&'static str> {
    let doc = view.toml(file).and_then(Result::ok)?;
    if doc.get("alias").and_then(toml::Value::as_table).is_some_and(|t| !t.is_empty()) {
        return Some("declares cargo aliases, a way to run the benches by another name");
    }
    let runner = doc
        .get("target")
        .and_then(toml::Value::as_table)
        .is_some_and(|t| t.values().filter_map(toml::Value::as_table).any(|t| t.contains_key("runner")));
    runner.then_some("declares a target `runner`, which runs every binary cargo starts through it")
}

/// Why the crate whose manifest is `manifest` in `dir` has a runner of its
/// own, or `None` when it is a library or a cdylib only.
fn own_binary(view: &View<'_>, dir: &Path, manifest: &Path) -> Option<&'static str> {
    let doc = view.toml(manifest).and_then(Result::ok);
    let has_table = |key: &str| {
        doc.and_then(|d| d.get(key))
            .and_then(toml::Value::as_array)
            .is_some_and(|a| !a.is_empty())
    };
    let auto = |key: &str| {
        doc.and_then(|d| d.get("package"))
            .and_then(|p| p.get(key))
            .and_then(toml::Value::as_bool)
            != Some(false)
    };
    if has_table("bin") {
        return Some("declares a `[[bin]]` of its own");
    }
    if has_table("bench") {
        return Some("declares a `[[bench]]` of its own");
    }
    if has_table("example") {
        return Some("declares an `[[example]]` of its own");
    }
    if auto("autobins") && view.is_file(dir.join("src/main.rs")) {
        return Some("is a crate with a `src/main.rs` of its own");
    }
    if auto("autobins") && view.is_dir(dir.join("src/bin")) {
        return Some("is a crate with binaries of its own under `src/bin/`");
    }
    if auto("autobenches") && view.is_dir(dir.join("benches")) {
        return Some("is a crate with cargo benches of its own under `benches/`");
    }
    if auto("autoexamples") && view.is_dir(dir.join("examples")) {
        return Some("is a crate with examples of its own under `examples/`");
    }
    None
}

/// The lint.
pub struct ABenchRunsOnlyOnTheGeneratedDriver;

impl Lint for ABenchRunsOnlyOnTheGeneratedDriver {
    fn name(&self) -> &'static str {
        NAME
    }

    fn description(&self) -> &'static str {
        "a bench tree carries no runner of its own: the driver `mock bench run` generates is the only one"
    }

    fn source_only(&self) -> bool {
        false
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }
}

impl RepoLint for ABenchRunsOnlyOnTheGeneratedDriver {
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError> {
        let bench_dir = ctx.mock_dir.join("benches");
        let tree = bench_dir
            .strip_prefix(ctx.repo_root)
            .unwrap_or(&bench_dir)
            .display()
            .to_string();
        bench_runners(ctx.view(), &bench_dir)
            .into_iter()
            .map(|r| {
                let mut e = LintError::error(
                    tree.clone(),
                    0,
                    NAME,
                    format!(
                        "{r}. The generated driver is the only bench runner: port what this does \
                         into the hooks library (`src/lib.rs` at the bench root, `pub fn hooks()`) \
                         and the bench.toml files, then delete it."
                    ),
                );
                e.path = Some(r.path.display().to_string());
                e
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Reading;

    fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        for (path, body) in files {
            let p = tmp.path().join("mock/benches").join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
        tmp
    }

    fn runners(tmp: &tempfile::TempDir) -> Vec<String> {
        let reading = Reading::new(tmp.path());
        bench_runners(&reading.view(), &tmp.path().join("mock/benches"))
            .into_iter()
            .map(|r| r.path.display().to_string())
            .collect()
    }

    const LIB: &str = "[package]\nname = \"s\"\nversion = \"0.0.0\"\n";

    /// The tree the generated driver runs: bench.toml files, an arm with a
    /// generated manifest, an arm with its own cdylib manifest, a support
    /// library and the hooks library. Nothing in it is a runner.
    #[test]
    fn a_tree_the_generated_driver_runs_has_no_runner() {
        let tmp = tree(&[
            ("bench.toml", "[timing]\n"),
            ("src/lib.rs", "pub fn hooks() {}\n"),
            ("hash/bench.toml", "arms = [\"fnv\"]\n"),
            ("hash/arms/fnv/src/lib.rs", ""),
            ("hash/arms/own/Cargo.toml", "[package]\nname = \"own\"\nversion = \"0.0.0\"\n[lib]\ncrate-type = [\"cdylib\"]\n"),
            ("hash/arms/own/src/lib.rs", "#![no_std]\n"),
            ("support/mix/Cargo.toml", LIB),
            ("support/mix/src/lib.rs", ""),
            ("support/mix/tests/t.rs", ""),
            ("target/debug/build.sh", ""),
            // Not runners: a data file with no extension and no shebang, a
            // cargo config with no aliases, a dot directory other than `.cargo`.
            ("support/mix/fixtures/LICENSE", "MPL-2.0\n"),
            (".cargo/config.toml", "[build]\nrustflags = []\n"),
            (".git/hooks/pre-commit", "#!/bin/sh\n"),
        ]);
        assert_eq!(runners(&tmp), Vec::<String>::new());
    }

    /// Every runner shape is found, each by its own file: a root driver crate
    /// (the old escape hatch, which is what three consumer trees carried),
    /// a root `main`, a member crate with a `main`, `[[bin]]`, `src/bin/`, a
    /// cargo bench, and a script.
    #[test]
    fn every_runner_shape_is_found() {
        let tmp = tree(&[
            ("Cargo.toml", "[package]\nname = \"benches\"\nversion = \"0.0.0\"\n"),
            ("src/main.rs", "fn main() {}\n"),
            ("engine_vs_std/Cargo.toml", LIB),
            ("engine_vs_std/src/main.rs", "fn main() {}\n"),
            ("declared/Cargo.toml", "[package]\nname = \"d\"\nversion = \"0.0.0\"\n[[bin]]\nname = \"x\"\npath = \"x.rs\"\n"),
            ("bins/Cargo.toml", LIB),
            ("bins/src/bin/x.rs", "fn main() {}\n"),
            ("crit/Cargo.toml", LIB),
            ("crit/benches/b.rs", ""),
            ("stage.sh", "#!/bin/sh\n"),
            ("frame_wide/occupancy.sh", "#!/bin/sh\n"),
            ("tools/plot.py", "print(1)\n"),
            ("tools/run.nu", "ls\n"),
            ("tools/run-all", "#!/usr/bin/env nutshell\n"),
            ("Makefile", "all:\n"),
            ("member/justfile", "run:\n"),
            ("ex/Cargo.toml", LIB),
            ("ex/examples/e.rs", "fn main() {}\n"),
            ("tabled/Cargo.toml", "[package]\nname = \"t\"\nversion = \"0.0.0\"\n[[example]]\nname = \"e\"\npath = \"e.rs\"\n"),
            (".cargo/config.toml", "[alias]\nbench-all = \"run --bin x\"\n"),
            ("member/.cargo/config.toml", "[target.x86_64-unknown-linux-gnu]\nrunner = \"taskset -c 2\"\n"),
            ("tools/run.fish", "#!/usr/bin/env fish\n"),
            ("tools/bench.pl", "#!/usr/bin/perl\n"),
        ]);
        assert_eq!(runners(&tmp), [
            ".cargo/config.toml",
            "Cargo.toml",
            "Makefile",
            "bins/Cargo.toml",
            "crit/Cargo.toml",
            "declared/Cargo.toml",
            "engine_vs_std/Cargo.toml",
            "ex/Cargo.toml",
            "frame_wide/occupancy.sh",
            "member/.cargo/config.toml",
            "member/justfile",
            "src/main.rs",
            "stage.sh",
            "tabled/Cargo.toml",
            "tools/bench.pl",
            "tools/plot.py",
            "tools/run-all",
            "tools/run.fish",
            "tools/run.nu",
        ]);
    }

    /// A crate that switched auto-discovery off has no binary from its
    /// `src/main.rs`, and a repository with no bench tree has nothing to say.
    #[test]
    fn autobins_off_and_an_absent_tree_find_nothing() {
        let tmp = tree(&[
            ("lib/Cargo.toml", "[package]\nname = \"l\"\nversion = \"0.0.0\"\nautobins = false\n"),
            ("lib/src/main.rs", ""),
        ]);
        assert_eq!(runners(&tmp), Vec::<String>::new());
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(runners(&empty), Vec::<String>::new());
    }
}
