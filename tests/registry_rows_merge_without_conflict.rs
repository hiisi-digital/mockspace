//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Two branches that each add a row to the same spot of a registry file merge.
//!
//! Every scenario runs the real binary and real git in a scratch repository
//! that `cargo mock` has wired, with `cargo mock` shimmed to this build, so
//! what is asserted is what a consumer's clone does at `git merge`: the
//! `.gitattributes` line, the driver in the clone's config, and the
//! `merge-registry` subcommand git calls through it.
//!
//! The control carries the rest. With the driver's config removed the same two
//! branches conflict as text, which is what shows the fixture makes a conflict
//! at all: a clean merge over a fixture that never conflicted would pass for any
//! reason.

mod common;

use std::fs;

use common::{Repo, write};

const FILE: &str = "mock/registry/probe.toml";

fn row(id: &str, text: &str) -> String {
    format!("[[probe]]\nid = \"{id}\"\ntext = \"{text}\"\n")
}

/// A repository with a registry of two rows committed and the wiring in place,
/// on `dev`.
fn repo() -> Repo {
    let r = Repo::new();
    write(
        &r.root.join("mock/mockspace.toml"),
        "project_name = \"fixture\"\ncrate_prefix = \"fixture\"\n\n[[registry.namespace]]\nkey = \"probe\"\ntitle = \"Probes\"\ndescription = \"Something measured.\"\n\n[[registry.namespace.field]]\nname = \"text\"\ntype = \"string\"\n",
    );
    write(
        &r.root.join(FILE),
        &format!("# the probes\n\n{}\n{}", row("a", "1"), row("b", "2")),
    );
    r.generate();
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "--no-verify", "-m", "chore: add a registry"]);
    r
}

fn append(r: &Repo, file: &str, branch: &str, add: &str) {
    let path = r.root.join(file);
    let text = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("{text}\n{add}")).unwrap();
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "--no-verify", "-m", &format!("chore: edit on {branch}")]);
}

/// Two branches off `dev`, each appending one row to `file` (the same spot).
fn two_additions(r: &Repo, file: &str) {
    r.git(&["switch", "-q", "-c", "a"]);
    append(r, file, "a", &row("from-a", "x"));
    r.git(&["switch", "-q", "dev"]);
    append(r, file, "dev", &row("from-dev", "y"));
}

fn merge(r: &Repo) -> std::process::Output {
    r.git_out(&["merge", "a", "-m", "chore: merge a"])
}

#[test]
fn control_without_the_driver_two_rows_added_at_the_same_spot_conflict() {
    let r = repo();
    two_additions(&r, FILE);
    r.git(&["config", "--local", "--remove-section", "merge.mockspace-registry"]);
    let merged = merge(&r);
    assert!(
        !merged.status.success(),
        "the fixture must conflict without the driver: {}",
        Repo::failure(&merged)
    );
    assert_eq!(r.unmerged(), [FILE]);
}

#[test]
fn two_rows_added_at_the_same_spot_both_stay_with_no_conflict() {
    let r = repo();
    two_additions(&r, FILE);
    let merged = merge(&r);
    assert!(merged.status.success(), "{}", Repo::failure(&merged));
    assert!(r.unmerged().is_empty());
    let text = fs::read_to_string(r.root.join(FILE)).unwrap();
    assert!(!text.contains("<<<<") && !text.contains(">>>>"), "{text}");
    assert!(text.starts_with("# the probes\n\n"), "{text}");
    let at = |id: &str| {
        text.find(&format!("id = \"{id}\""))
            .unwrap_or_else(|| panic!("{id} missing: {text}"))
    };
    assert!(
        at("a") < at("b") && at("b") < at("from-a") && at("from-a") < at("from-dev"),
        "{text}"
    );
    // the registry file itself is committed as merged; the generated page that lists
    // its rows is the regeneration hook's business, not the driver's
    assert_eq!(r.git(&["status", "--porcelain", "--", FILE]), "", "{text}");
}

#[test]
fn the_merged_file_is_the_same_whichever_branch_is_merged_into_which() {
    let one = repo();
    two_additions(&one, FILE);
    assert!(merge(&one).status.success());
    let two = repo();
    two_additions(&two, FILE);
    two.git(&["switch", "-q", "a"]);
    assert!(
        two.git_out(&["merge", "dev", "-m", "chore: merge dev"])
            .status
            .success()
    );
    assert_eq!(
        fs::read_to_string(one.root.join(FILE)).unwrap(),
        fs::read_to_string(two.root.join(FILE)).unwrap()
    );
}

#[test]
fn the_row_is_what_counts_and_the_file_is_not() {
    let r = repo();
    // a nested file under another name, and a row added to it on each side
    write(
        &r.root.join("mock/registry/deep/er/anything.toml"),
        &row("n", "0"),
    );
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "--no-verify", "-m", "chore: add a nested file"]);
    two_additions(&r, "mock/registry/deep/er/anything.toml");
    let merged = merge(&r);
    assert!(merged.status.success(), "{}", Repo::failure(&merged));
    let text = fs::read_to_string(r.root.join("mock/registry/deep/er/anything.toml")).unwrap();
    assert!(
        text.contains("from-a") && text.contains("from-dev") && !text.contains("<<<<"),
        "{text}"
    );
}

/// The registry as the merged tree loads it, and the rows' key paths.
fn loaded(r: &Repo) -> mockspace::registry::Registry {
    let mock = r.root.join("mock");
    let cfg = mockspace::config::Config::from_dir(&mock);
    mockspace::registry::load_registry(&mock, &cfg.registry_namespaces)
}

/// Each branch adds a row of the same table to a file of its own, in different directories.
fn additions_to_different_files(r: &Repo, id_a: &str, id_dev: &str) {
    r.git(&["switch", "-q", "-c", "a"]);
    write(&r.root.join("mock/registry/from/a/rows.toml"), &row(id_a, "x"));
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "--no-verify", "-m", "chore: add a file on a"]);
    r.git(&["switch", "-q", "dev"]);
    write(&r.root.join("mock/registry/dev.toml"), &row(id_dev, "y"));
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "--no-verify", "-m", "chore: add a file on dev"]);
}

#[test]
fn rows_added_to_different_files_under_one_table_all_load_after_the_merge() {
    let r = repo();
    additions_to_different_files(&r, "from-a", "from-dev");
    let merged = merge(&r);
    assert!(merged.status.success(), "{}", Repo::failure(&merged));
    let reg = loaded(&r);
    let want: Vec<String> = ["a", "b", "from-a", "from-dev"]
        .iter()
        .map(|i| format!("probe::{i}"))
        .collect();
    assert_eq!(reg.by_namespace["probe"], want);
    assert!(reg.duplicates.is_empty(), "{:?}", reg.duplicates);
}

/// The driver sees one file at a time, so one id added in two files by two branches merges
/// cleanly as text. What refuses it is the registry, naming both files.
#[test]
fn one_key_path_added_to_two_files_by_two_branches_is_refused_after_the_merge() {
    let r = repo();
    additions_to_different_files(&r, "same", "same");
    let merged = merge(&r);
    assert!(merged.status.success(), "{}", Repo::failure(&merged));
    let reg = loaded(&r);
    let files: Vec<String> = reg.duplicates["probe::same"]
        .iter()
        .map(|p| p.strip_prefix(r.root.join("mock")).unwrap().display().to_string())
        .collect();
    assert_eq!(files, ["registry/dev.toml", "registry/from/a/rows.toml"]);
    let found = mockspace::registry::validate(&[], &reg);
    assert!(
        found.iter().any(|f| f.kind == "duplicate-id" && f.message.contains("probe::same")),
        "{:?}",
        found.iter().map(|f| &f.message).collect::<Vec<_>>()
    );
}

#[test]
fn a_row_changed_two_ways_still_conflicts_and_only_that_row_is_marked() {
    let r = repo();
    r.git(&["switch", "-q", "-c", "a"]);
    let path = r.root.join(FILE);
    let base = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        base.replace("text = \"2\"", "text = \"two, edited on a\""),
    )
    .unwrap();
    r.git(&["commit", "-q", "--no-verify", "-am", "chore: edit on a"]);
    r.git(&["switch", "-q", "dev"]);
    fs::write(
        &path,
        base.replace("text = \"2\"", "text = \"two, edited on dev\""),
    )
    .unwrap();
    r.git(&["commit", "-q", "--no-verify", "-am", "chore: edit on dev"]);

    let merged = merge(&r);
    assert!(!merged.status.success(), "{}", Repo::failure(&merged));
    assert_eq!(r.unmerged(), [FILE]);
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(text.matches("<<<<<<< ours").count(), 1, "{text}");
    assert!(
        text.contains("text = \"two, edited on a\"")
            || text.contains("text = \"two, edited on dev\""),
        "{text}"
    );
    let marked = &text[text.find("<<<<<<<").unwrap() .. text.find(">>>>>>>").unwrap()];
    assert!(
        !marked.contains("id = \"a\""),
        "row a is outside the markers: {text}"
    );
    assert!(text.contains(&row("a", "1")), "{text}");
}

#[test]
fn the_wiring_is_written_once_and_a_second_run_changes_nothing() {
    let r = repo();
    let attrs = fs::read_to_string(r.root.join(".gitattributes")).unwrap();
    let line = "/mock/registry/**/*.toml merge=mockspace-registry";
    assert_eq!(attrs.matches(line).count(), 1, "{attrs}");
    assert!(
        attrs.contains("/docs/* merge=mockspace-generated"),
        "the documents line stays: {attrs}"
    );
    let driver = r.git(&["config", "--local", "merge.mockspace-registry.driver"]);
    assert!(
        driver.contains("cargo mock merge-registry %O %A %B %P %L"),
        "{driver}"
    );
    r.generate();
    r.generate();
    assert_eq!(
        fs::read_to_string(r.root.join(".gitattributes")).unwrap(),
        attrs
    );
    assert_eq!(
        r.git(&["config", "--local", "merge.mockspace-registry.driver"]),
        driver
    );
    assert_eq!(
        r.git(&["config", "--local", "--get-all", "merge.mockspace-registry.driver"])
            .lines()
            .count(),
        1
    );
}

#[test]
fn a_clone_where_the_engine_cannot_be_run_merges_as_git_would() {
    let r = repo();
    two_additions(&r, FILE);
    // a PATH with no cargo: the driver's fallback is git's own text merge, so
    // this conflicts exactly as the control does, and never merges wrongly
    let out = r
        .cmd("git")
        .env("MS_NO_CARGO", "1")
        .args(["merge", "a", "-m", "chore: merge a"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "{}", Repo::failure(&out));
    assert_eq!(r.unmerged(), [FILE]);
}

/// Branch `a` and `dev` each change row `b` of the registry file, in two ways.
fn row_changed_two_ways(r: &Repo) {
    let path = r.root.join(FILE);
    r.git(&["switch", "-q", "-c", "a"]);
    let base = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        base.replace("text = \"2\"", "text = \"two, edited on a\""),
    )
    .unwrap();
    r.git(&["commit", "-q", "--no-verify", "-am", "chore: edit on a"]);
    r.git(&["switch", "-q", "dev"]);
    fs::write(
        &path,
        base.replace("text = \"2\"", "text = \"two, edited on dev\""),
    )
    .unwrap();
    r.git(&["commit", "-q", "--no-verify", "-am", "chore: edit on dev"]);
}

/// Ask git for markers `size` long on the registry file, the way a clone's own attributes do.
fn marker_size(r: &Repo, size: usize) {
    let attrs = r.root.join(".git/info/attributes");
    write(&attrs, &format!("{FILE} conflict-marker-size={size}\n"));
}

/// The number of `<` opening the line that starts a conflict.
fn opening_marker_len(text: &str) -> usize {
    let line = text
        .lines()
        .find(|l| l.starts_with('<'))
        .unwrap_or_else(|| panic!("no conflict marker: {text}"));
    line.chars().take_while(|c| *c == '<').count()
}

#[test]
fn a_conflict_is_marked_as_long_as_git_asks_for() {
    let r = repo();
    row_changed_two_ways(&r);
    marker_size(&r, 11);
    let merged = merge(&r);
    assert!(!merged.status.success(), "{}", Repo::failure(&merged));
    let text = fs::read_to_string(r.root.join(FILE)).unwrap();
    assert_eq!(opening_marker_len(&text), 11, "{text}");
    assert!(
        text.lines()
            .any(|l| l.starts_with(&">".repeat(11)) && !l.starts_with(&">".repeat(12))),
        "the closing marker is as long: {text}"
    );
}

#[test]
fn a_conflict_with_no_attribute_is_marked_seven_long() {
    // the control for the arm above: the length is read from git, not always eleven
    let r = repo();
    row_changed_two_ways(&r);
    let merged = merge(&r);
    assert!(!merged.status.success(), "{}", Repo::failure(&merged));
    let text = fs::read_to_string(r.root.join(FILE)).unwrap();
    assert_eq!(opening_marker_len(&text), 7, "{text}");
}

#[test]
fn the_text_fallback_marks_as_long_as_git_asks_for_too() {
    let r = repo();
    two_additions(&r, FILE);
    marker_size(&r, 11);
    let out = r
        .cmd("git")
        .env("MS_NO_CARGO", "1")
        .args(["merge", "a", "-m", "chore: merge a"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "{}", Repo::failure(&out));
    let text = fs::read_to_string(r.root.join(FILE)).unwrap();
    assert_eq!(opening_marker_len(&text), 11, "{text}");
}

/// The registry file with CRLF line endings on `dev`, committed.
fn crlf_repo() -> Repo {
    let r = repo();
    let path = r.root.join(FILE);
    let text = fs::read_to_string(&path).unwrap();
    fs::write(&path, text.replace('\n', "\r\n")).unwrap();
    r.git(&["commit", "-q", "--no-verify", "-am", "chore: crlf"]);
    r
}

/// True when every line of `text` ends in CRLF and no bare LF is left.
fn all_crlf(text: &str) -> bool {
    text.matches('\n').count() == text.matches("\r\n").count()
}

#[test]
fn rows_added_to_a_crlf_file_merge_with_crlf_throughout() {
    let r = crlf_repo();
    let crlf = |s: String| s.replace('\n', "\r\n");
    let add = |branch: &str, id: &str| {
        let path = r.root.join(FILE);
        let text = fs::read_to_string(&path).unwrap();
        fs::write(&path, format!("{text}\r\n{}", crlf(row(id, "x")))).unwrap();
        r.git(&["commit", "-q", "--no-verify", "-am", &format!("chore: edit on {branch}")]);
    };
    r.git(&["switch", "-q", "-c", "a"]);
    add("a", "from-a");
    r.git(&["switch", "-q", "dev"]);
    add("dev", "from-dev");
    let merged = merge(&r);
    assert!(merged.status.success(), "{}", Repo::failure(&merged));
    let text = fs::read_to_string(r.root.join(FILE)).unwrap();
    assert!(
        text.contains("from-a") && text.contains("from-dev"),
        "{text:?}"
    );
    assert!(all_crlf(&text), "a bare LF got into a CRLF file: {text:?}");
}

#[test]
fn a_conflict_in_a_crlf_file_has_crlf_markers() {
    let r = crlf_repo();
    row_changed_two_ways(&r);
    let merged = merge(&r);
    assert!(!merged.status.success(), "{}", Repo::failure(&merged));
    let text = fs::read_to_string(r.root.join(FILE)).unwrap();
    assert!(text.contains("<<<<<<< ours"), "{text:?}");
    assert!(
        all_crlf(&text),
        "a marker line ended in a bare LF: {text:?}"
    );
}
