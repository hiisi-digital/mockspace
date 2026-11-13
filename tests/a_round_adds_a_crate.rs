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

/// What the generated write guard said.
#[derive(Debug)]
enum Verdict {
    Admitted,
    Refused(String),
}

/// The generated write guard's verdict on `tool` carrying `input`.
///
/// Only the two shapes the guard prints count: the bare allow, or a deny with its reason.
/// Anything else, a non-zero exit or output of another shape, fails the test, so a hook that
/// crashed never reads as one that admitted.
fn guard(r: &Repo, tool: &str, input: serde_json::Value) -> Verdict {
    let payload = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": tool,
        "tool_input": input,
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
    assert!(
        out.status.success(),
        "the guard itself failed: {}",
        said(&out)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("the guard printed no decision ({e}): {}", said(&out)));
    let hso = &json["hookSpecificOutput"];
    match hso.get("permissionDecision").and_then(|d| d.as_str()) {
        None if hso.as_object().is_some_and(|o| o.len() == 1) => Verdict::Admitted,
        Some("deny") => {
            Verdict::Refused(
                hso["permissionDecisionReason"]
                    .as_str()
                    .unwrap_or("")
                    .into(),
            )
        },
        _ => panic!("the guard printed a decision of another shape: {stdout}"),
    }
}

fn lib_path(r: &Repo) -> String {
    r.root
        .join(format!("mock/crates/{CRATE}/src/lib.rs"))
        .display()
        .to_string()
}

/// An agent's Write of `content` to the new crate's `lib.rs`.
fn write_lib(r: &Repo, content: &str) -> Verdict {
    guard(
        r,
        "Write",
        serde_json::json!({ "file_path": lib_path(r), "content": content }),
    )
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
    let v = write_lib(&r, DOCS_ONLY);
    assert!(matches!(v, Verdict::Admitted), "{v:?}");
}

/// And refuses the twin carrying one item, in the lint's own words, which only the
/// answer from `check-edit` carries.
#[test]
fn the_write_guard_refuses_one_item_in_the_doc_phase() {
    let r = doc_phase();
    match write_lib(&r, ONE_ITEM) {
        Verdict::Refused(why) => assert!(why.contains("would declare items"), "{why}"),
        v => panic!("{v:?}"),
    }
}

/// An Edit is judged by the file it would leave: adding an item to a `lib.rs` of docs
/// is refused, and adding a doc line is admitted.
#[test]
fn the_write_guard_judges_an_edit_by_the_file_it_leaves() {
    let r = doc_phase();
    scaffold(&r, DOCS_ONLY);
    let edit = |new: &str| {
        guard(
            &r,
            "Edit",
            serde_json::json!({
                "file_path": lib_path(&r),
                "old_string": "#![no_std]\n",
                "new_string": new,
            }),
        )
    };
    let docs = edit("#![no_std]\n//! One more line of the crate's docs.\n");
    let item = edit("#![no_std]\npub fn born() {}\n");
    assert!(matches!(docs, Verdict::Admitted), "{docs:?}");
    match item {
        Verdict::Refused(why) => assert!(why.contains("would declare items"), "{why}"),
        v => panic!("{v:?}"),
    }
}

/// A Bash command writing the file carries no text to ask about, so it keeps the phase rule.
#[test]
fn a_bash_write_of_the_lib_rs_keeps_the_phase_rule() {
    let r = doc_phase();
    scaffold(&r, DOCS_ONLY);
    let v = guard(
        &r,
        "Bash",
        serde_json::json!({
            "command": format!("sed -i 's/x/y/' {}", lib_path(&r)),
        }),
    );
    match v {
        Verdict::Refused(why) => assert!(why.contains("not in IMPL phase"), "{why}"),
        v => panic!("{v:?}"),
    }
}

/// The phases other than DOC and IMPL, by name.
const OTHER_PHASES: [&str; 3] = ["TOPIC", "DRAFT", "CLOSED"];

/// `r`, opened in DOC, moved on to `phase`: TOPIC has no changelist, DRAFT the doc
/// changelist locked, CLOSED both locked.
fn move_to(r: &Repo, phase: &str) {
    match phase {
        "TOPIC" => {
            r.git(&["rm", "-q", DOC_CL]);
            r.git(&["commit", "-q", "--no-verify", "-m", "docs: back to the topic"]);
        },
        "DRAFT" => lock(r, DOC_CL),
        "CLOSED" => {
            lock(r, DOC_CL);
            write(&r.root.join(SRC_CL), "# Changelist\n");
            r.git(&["add", "-A"]);
            r.git(&["commit", "-q", "--no-verify", "-m", "docs: open the source changelist"]);
            lock(r, SRC_CL);
        },
        other => panic!("no phase {other}"),
    }
}

/// Outside DOC and IMPL the write guard still admits documentation and refuses an item.
#[test]
fn the_write_guard_admits_docs_and_refuses_an_item_in_every_other_phase() {
    for phase in OTHER_PHASES {
        let r = doc_phase();
        move_to(&r, phase);
        let docs = write_lib(&r, DOCS_ONLY);
        assert!(matches!(docs, Verdict::Admitted), "{phase}: {docs:?}");
        match write_lib(&r, ONE_ITEM) {
            Verdict::Refused(why) => assert!(why.contains("would declare items"), "{phase}: {why}"),
            v => panic!("{phase}: {v:?}"),
        }
    }
}

/// And the commit gate refuses the item in each of them.
#[test]
fn the_commit_gate_refuses_an_item_in_every_other_phase() {
    for phase in OTHER_PHASES {
        let r = doc_phase();
        move_to(&r, phase);
        scaffold(&r, ONE_ITEM);
        let out = commit_gate(&r);
        assert!(!out.status.success(), "{phase}: {}", said(&out));
        assert!(
            said(&out).contains("changelist-required"),
            "{phase}: {}",
            said(&out)
        );
    }
}

/// A crate added with no round open to add it, in TOPIC or CLOSED, is empty, and the
/// build gate refuses it. `no-empty-crate` is advisory by default, so the arm runs at
/// the severity the consumer that met this sets.
#[test]
fn a_crate_added_outside_an_open_round_is_refused_at_build() {
    for phase in ["TOPIC", "CLOSED"] {
        let r = doc_phase();
        move_to(&r, phase);
        scaffold(&r, DOCS_ONLY);
        let out = r
            .cmd(BIN)
            .current_dir(r.root.join("mock"))
            .args(["--lint-only"])
            .output()
            .unwrap();
        assert!(!out.status.success(), "{phase}: {}", said(&out));
        assert!(
            said(&out).contains("no-empty-crate"),
            "{phase}: {}",
            said(&out)
        );
    }
}

#[path = "common/ambient.rs"]
mod ambient;
