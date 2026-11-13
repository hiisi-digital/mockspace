//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A design round can add a crate.
//!
//! The round's doc phase commits the new crate's manifest and a `lib.rs` holding
//! the crate's documentation only, and its source phase commits the items. Before
//! this the sequence had no way through: cargo cannot read a workspace whose
//! member has no `src/lib.rs`, `no-empty-crate` refused a `lib.rs` of docs, any
//! item in it needed the source phase, and the write guard refused writing a
//! `.rs` file at all outside IMPL.
//!
//! Every arm runs the real binary in a scratch consumer wired by `cargo mock`:
//! the commit gate as the pre-commit hook runs it, and the generated write guard
//! fed the payload an agent's write sends. `no-empty-crate` is at error, as the
//! consumer that met this sets it.

mod common;

use std::fs;
use std::process::{Output, Stdio};

use common::{BIN, Repo, write};

const CRATE: &str = "fixture-born";
const DOCS_ONLY: &str = "//! The crate a round adds, in its own words.\n#![no_std]\n";
const ONE_ITEM: &str =
    "//! The crate a round adds, in its own words.\n#![no_std]\n\npub fn born() {}\n";
const DOC_CL: &str = "mock/design_rounds/202611130000_changelist.doc.md";
const SRC_CL: &str = "mock/design_rounds/202611130000_changelist.src.md";

/// A consumer with a round open in DOC whose changelist adds `fixture-born`.
fn doc_phase() -> Repo {
    let r = Repo::new();
    let config = r.root.join("mock/mockspace.toml");
    let mut text = fs::read_to_string(&config).unwrap();
    text.push_str(
        "\n[lints.no-empty-crate]\ncommit = \"error\"\nbuild = \"error\"\npush = \"error\"\n",
    );
    write(&config, &text);
    write(
        &r.root.join("mock/design_rounds/202611130000_topic.born.md"),
        "# A crate for the born thing\n",
    );
    write(
        &r.root.join(DOC_CL),
        &format!("# Changelist\n\n## Changes\n\nA new crate, `{CRATE}`, holds the born thing.\n"),
    );
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "--no-verify", "-m", "docs: open the round"]);
    r
}

/// The crate's manifest, its `lib.rs` holding `lib`, and the workspace naming it, staged.
fn scaffold(r: &Repo, lib: &str) {
    write(
        &r.root.join(format!("mock/crates/{CRATE}/Cargo.toml")),
        &format!("[package]\nname = \"{CRATE}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
    );
    write(&r.root.join(format!("mock/crates/{CRATE}/src/lib.rs")), lib);
    write(
        &r.root.join("mock/Cargo.toml"),
        &format!("[workspace]\nmembers = [\"crates/{CRATE}\"]\nresolver = \"2\"\n"),
    );
    r.git(&["add", "-A"]);
}

/// The commit gate, as the pre-commit hook runs it.
fn commit_gate(r: &Repo) -> Output {
    r.cmd(BIN)
        .current_dir(r.root.join("mock"))
        .args(["--lint-only", "--commit"])
        .output()
        .unwrap()
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Moves a changelist to its locked name, as `cargo mock lock` does, and commits.
fn lock(r: &Repo, cl: &str) {
    let locked = cl.replace(".md", ".lock.md");
    r.git(&["mv", cl, &locked]);
    r.git(&["commit", "-q", "--no-verify", "-m", "docs: lock"]);
}

/// The generated write guard's verdict on an agent writing `content` to the crate's `lib.rs`.
fn guard_admits(r: &Repo, content: &str) -> (bool, String) {
    let lib = r.root.join(format!("mock/crates/{CRATE}/src/lib.rs"));
    let payload = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Write",
        "tool_input": { "file_path": lib.display().to_string(), "content": content },
    })
    .to_string();
    let mut child = r
        .cmd("bash")
        .arg(r.root.join(".claude/hooks/mockspace-write-guard.sh"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let text = said(&out);
    (!text.contains("permissionDecision"), text)
}

/// The control for the gate arms: with the doc changelist naming no crate, the same
/// scaffold is refused by `no-empty-crate`, so the arm that passes passes for the round.
#[test]
fn control_a_round_that_does_not_name_the_crate_leaves_it_empty() {
    let r = doc_phase();
    write(
        &r.root.join(DOC_CL),
        "# Changelist\n\n## Changes\n\nNothing about crates.\n",
    );
    r.git(&["commit", "-q", "--no-verify", "-am", "docs: name nothing"]);
    scaffold(&r, DOCS_ONLY);
    let out = commit_gate(&r);
    assert!(!out.status.success(), "{}", said(&out));
    assert!(said(&out).contains("no-empty-crate"), "{}", said(&out));
}

/// The doc phase commits the manifest and a `lib.rs` of documentation.
#[test]
fn the_doc_phase_commits_the_manifest_and_a_lib_rs_of_documentation() {
    let r = doc_phase();
    scaffold(&r, DOCS_ONLY);
    let out = commit_gate(&r);
    assert!(out.status.success(), "{}", said(&out));
}

/// The twin of the arm above, one item apart: `changelist-required` refuses it.
#[test]
fn the_doc_phase_refuses_one_item_in_the_new_lib_rs() {
    let r = doc_phase();
    scaffold(&r, ONE_ITEM);
    let out = commit_gate(&r);
    assert!(!out.status.success(), "{}", said(&out));
    assert!(said(&out).contains("changelist-required"), "{}", said(&out));
}

/// The whole sequence: docs in DOC, the lock, the items in IMPL.
#[test]
fn the_source_phase_commits_the_items() {
    let r = doc_phase();
    scaffold(&r, DOCS_ONLY);
    r.git(&["commit", "-q", "--no-verify", "-m", "docs: scaffold the crate"]);
    lock(&r, DOC_CL);
    let draft = commit_gate(&r);
    assert!(draft.status.success(), "DRAFT: {}", said(&draft));
    write(
        &r.root.join(SRC_CL),
        "# Changelist\n\n## Changes\n\nThe born thing.\n",
    );
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "--no-verify", "-m", "docs: open the source changelist"]);
    scaffold(&r, ONE_ITEM);
    let out = commit_gate(&r);
    assert!(out.status.success(), "IMPL: {}", said(&out));
}

/// With both changelists locked, a crate the round never populated is empty again.
#[test]
fn a_round_cannot_lock_over_a_crate_it_left_empty() {
    let r = doc_phase();
    scaffold(&r, DOCS_ONLY);
    r.git(&["commit", "-q", "--no-verify", "-m", "docs: scaffold the crate"]);
    lock(&r, DOC_CL);
    write(&r.root.join(SRC_CL), "# Changelist\n");
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "--no-verify", "-m", "docs: open the source changelist"]);
    lock(&r, SRC_CL);
    let out = commit_gate(&r);
    assert!(!out.status.success(), "{}", said(&out));
    assert!(said(&out).contains("no-empty-crate"), "{}", said(&out));
}

/// The write guard lets an agent write the crate's documentation in DOC.
#[test]
fn the_write_guard_admits_a_lib_rs_of_documentation_in_the_doc_phase() {
    let r = doc_phase();
    let (admitted, text) = guard_admits(&r, DOCS_ONLY);
    assert!(admitted, "{text}");
}

/// And refuses the twin carrying one item, naming the phase.
#[test]
fn the_write_guard_refuses_one_item_in_the_doc_phase() {
    let r = doc_phase();
    let (admitted, text) = guard_admits(&r, ONE_ITEM);
    assert!(!admitted, "{text}");
    assert!(text.contains("not in IMPL phase"), "{text}");
}
