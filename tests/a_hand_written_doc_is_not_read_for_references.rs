//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The dangling-reference scan reads what generation wrote, not every page in the docs directory.
//! A page someone wrote there by hand renders nothing, so a reference in it is its author's text,
//! such as a page teaching the syntax by example, and failing the run over it failed every run of
//! a repository that keeps one.
//!
//! Two arms over one reference that resolves to nothing. The template arm is the control: the same
//! text in a template must still fail the run, or the hand-written arm passing says nothing about
//! the scan.

mod common;

use common::{Repo, write};

const CONFIG: &str = "project_name = \"fixture\"\ncrate_prefix = \"fixture\"\n";
const DANGLING: &str = "See {{ mock::no_such_page }} for the rest.\n";

fn generate(r: &Repo) -> std::process::Output {
    r.cmd(common::BIN)
        .current_dir(r.root.join("mock"))
        .arg("--generate-only")
        .output()
        .expect("the binary runs")
}

#[test]
fn a_dangling_reference_in_a_template_still_fails_the_run() {
    let r = Repo::new();
    write(&r.root.join("mock/mockspace.toml"), CONFIG);
    write(&r.root.join("mock/ALPHA.md.tmpl"), DANGLING);
    let out = generate(&r);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "a generated page with a dangling reference passed:\n{err}");
    assert!(err.contains("dangling-reference"), "failed for another reason:\n{err}");
}

#[test]
fn a_reference_in_a_hand_written_page_is_left_to_its_author() {
    let r = Repo::new();
    write(&r.root.join("mock/mockspace.toml"), CONFIG);
    write(&r.root.join("docs/SYNTAX.md"), &format!("# Syntax\n\n{DANGLING}"));
    let out = generate(&r);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "a hand-written page failed the run:\n{err}");
    assert!(!err.contains("dangling-reference"), "the hand-written page was read:\n{err}");
}
