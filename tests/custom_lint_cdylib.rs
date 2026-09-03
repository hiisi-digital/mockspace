//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! End-to-end validation of the runtime custom-lint cdylib path: generate the
//! cdylib from a repo's `mock/lints/*.rs`, build it against the real
//! `mockspace-lint-rules`, dlopen it, and confirm a real `Box<dyn Lint>`
//! crosses the boundary and dispatches.
//!
//! `#[ignore]` because it runs a `cargo build` (slow, and needs the toolchain).
//! Not manual-only, though: `tests/rust_e2e_test.sh` runs it with `--ignored`
//! every time `./test` does, which is the whole reason it exists. This test
//! sat broken for weeks with no runner ever reaching it; that file is the fix.
//! Catalogue: this guards the dissolve-proxy deal-breaker (custom lints must
//! work through the shared engine). Tracked with the launcher work.

use std::fs;

#[test]
#[ignore = "runs cargo build; run with --ignored"]
fn custom_lint_loads_and_dispatches_across_cdylib() {
    let tmp = tempfile::tempdir().unwrap();
    let mock = tmp.path().join("mock");
    let lints = mock.join("lints");
    fs::create_dir_all(&lints).unwrap();
    fs::write(mock.join("mockspace.toml"), "project_name = \"probe\"\n").unwrap();

    // a real lint using the same imports the consumer lints use.
    fs::write(
        lints.join("trivial.rs"),
        // The fixture is a string, so nothing type-checks it at compile time
        // and `#[ignore]` means nothing ran it. It sat broken from the commit
        // that split `Lint` into `Lint` + `CrateLint`, which updated this
        // file's prose and not the source inside it. That is the whole failure
        // mode: a test that reads as coverage, compiles, and cannot pass.
        r#"use mockspace::{CrateLint, Lint, LintContext, LintError};
pub fn lint() -> Box<dyn CrateLint> { Box::new(Trivial) }
struct Trivial;
impl Lint for Trivial {
    fn name(&self) -> &'static str { "trivial-cdylib-probe" }
}
impl CrateLint for Trivial {
    fn check(&self, _ctx: &LintContext) -> Vec<LintError> { Vec::new() }
}
"#,
    )
    .unwrap();

    let cfg = mockspace::config::Config::from_dir(&mock);
    // build the cdylib's mockspace-lint-rules from the workspace copy, renamed
    // to `mockspace` so the lint file compiles unchanged.
    let lint_rules = concat!(env!("CARGO_MANIFEST_DIR"), "/lint-rules");
    let dep = format!("{{ package = \"mockspace-lint-rules\", path = \"{lint_rules}\" }}");

    let loaded = mockspace::custom_lints::load(&cfg, &mock.join("mockspace.toml"), &dep)
        .expect("load ok")
        .expect("has custom lints");

    assert_eq!(loaded.pack.crate_lints.len(), 1);
    assert!(loaded.pack.workspace_lints.is_empty());
    assert!(loaded.pack.repo_lints.is_empty());
    assert!(loaded.pack.message_lints.is_empty());
    // dispatch across the cdylib boundary.
    assert_eq!(loaded.pack.crate_lints[0].name(), "trivial-cdylib-probe");
}

#[test]
#[ignore = "runs cargo; run with --ignored"]
fn a_manifest_with_two_patch_tables_is_accepted_and_the_unused_one_is_inert() {
    // The claim the two-spelling patch rests on, and nothing else in this repo
    // could answer it: the generator emits a `[patch]` table for both url
    // spellings of one repository, so in every real build one of the two names
    // a source nothing resolves to. If cargo objected to that, or tried to
    // fetch it, the fix would break every consumer instead of the one it
    // repairs.
    //
    // It was established by hand first, by adding the second table to a
    // consumer's generated manifest and watching the cdylib build, which is
    // exactly the check that has to become a test rather than a memory.
    //
    // No network: the unused table names a host that does not exist, and the
    // point is that cargo never reaches for it. A failure here is either a
    // parse error on the second table or an attempted fetch, and both are
    // loud.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn nothing() {}\n").unwrap();

    let lint_rules = concat!(env!("CARGO_MANIFEST_DIR"), "/lint-rules");
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[workspace]\n\n\
             [package]\nname = \"two-tables-probe\"\nversion = \"0.0.0\"\n\
             edition = \"2024\"\npublish = false\n\n\
             [dependencies]\n\
             mockspace = {{ package = \"mockspace-lint-rules\", path = \"{lint_rules}\" }}\n\n\
             [patch.\"https://example.invalid/mockspace.git\"]\n\
             mockspace-lint-rules = {{ path = \"{lint_rules}\" }}\n\n\
             [patch.\"ssh://git@example.invalid/mockspace.git\"]\n\
             mockspace-lint-rules = {{ path = \"{lint_rules}\" }}\n"
        ),
    )
    .unwrap();

    let out = std::process::Command::new("cargo")
        .arg("metadata")
        .arg("--format-version=1")
        .arg("--offline")
        .current_dir(root)
        .output()
        .expect("cargo runs");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "cargo refused a manifest carrying two patch tables: {err}"
    );
    // The two together, since a duplicate key and an attempted fetch are the
    // two ways this goes wrong and they report differently.
    assert!(
        !err.contains("duplicate key"),
        "the two tables collided: {err}"
    );
    assert!(
        !err.contains("example.invalid"),
        "cargo reached for a source nothing names: {err}"
    );
}

#[test]
#[ignore = "runs cargo; run with --ignored"]
fn cargo_does_reach_an_unreachable_host_when_something_names_it() {
    // What makes the case above non-vacuous, and without this it is not.
    //
    // That case asserts cargo never mentions the unreachable host, on the
    // reasoning that a patch table for a source nothing resolves to is inert.
    // The assertion is worth nothing unless cargo would have mentioned the host
    // had it reached for it, and the case cannot establish that about itself.
    // Its sibling negative control fires at TOML parse, which happens before
    // resolution, so it proves only that the tables are read.
    //
    // So this is the same manifest with the host moved from a patch table into
    // a dependency, where cargo must reach for it. It fails and names the host,
    // which is what the other case asserts the absence of.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn nothing() {}\n").unwrap();

    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\n\n\
         [package]\nname = \"reachable-probe\"\nversion = \"0.0.0\"\n\
         edition = \"2024\"\npublish = false\n\n\
         [dependencies]\n\
         nothing-real = { git = \"https://example.invalid/mockspace.git\" }\n",
    )
    .unwrap();

    let out = std::process::Command::new("cargo")
        .arg("metadata")
        .arg("--format-version=1")
        .arg("--offline")
        .current_dir(root)
        .output()
        .expect("cargo runs");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "cargo resolved a dependency on a host that does not exist, so the \
         inertness case above is asserting the absence of something that would \
         never have appeared"
    );
    assert!(
        err.contains("example.invalid"),
        "cargo failed without naming the host, so the other case's third \
         assertion cannot discriminate: {err}"
    );
}

#[test]
#[ignore = "runs cargo; run with --ignored"]
fn two_tables_naming_one_source_is_refused_which_is_why_they_are_deduped() {
    // The negative control for the case above, and the reason `spellings_of`
    // deduplicates rather than trusting its own arithmetic. Two `[patch]`
    // tables for the same url is a manifest cargo will not parse, so a future
    // third spelling that happens to collide with one of the two would stop
    // every `cargo mock` in the consumer repository.
    //
    // Without this, the case above passes whether or not the dedup exists and
    // says nothing about why it is there.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn nothing() {}\n").unwrap();

    let lint_rules = concat!(env!("CARGO_MANIFEST_DIR"), "/lint-rules");
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[workspace]\n\n\
             [package]\nname = \"dup-probe\"\nversion = \"0.0.0\"\n\
             edition = \"2024\"\npublish = false\n\n\
             [dependencies]\n\
             mockspace = {{ package = \"mockspace-lint-rules\", path = \"{lint_rules}\" }}\n\n\
             [patch.\"https://example.invalid/mockspace.git\"]\n\
             mockspace-lint-rules = {{ path = \"{lint_rules}\" }}\n\n\
             [patch.\"https://example.invalid/mockspace.git\"]\n\
             mockspace-lint-rules = {{ path = \"{lint_rules}\" }}\n"
        ),
    )
    .unwrap();

    let out = std::process::Command::new("cargo")
        .arg("metadata")
        .arg("--format-version=1")
        .arg("--offline")
        .current_dir(root)
        .output()
        .expect("cargo runs");
    assert!(
        !out.status.success(),
        "cargo accepted two tables naming one source, so the dedup guards \
         nothing and this file's other case proves less than it says"
    );
}
