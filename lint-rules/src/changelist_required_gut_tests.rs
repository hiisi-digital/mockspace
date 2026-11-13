//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A source file that held items and is cut down to a comment is a source change.
//!
//! Scaffolding passes outside IMPL because it declares nothing on both sides. Judged on the
//! new text alone, a file gutted to `// emptied` read as scaffolding, and so did a `lib.rs`
//! stub carrying the nuke marker, which then excused every removal in the crate. Each arm
//! runs the gate end to end on a committed repository in DOC, and the edit question the
//! write guard asks, so the two cannot drift.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

const LIB_ITEMS: &str = "//! The crate.\npub mod more;\npub fn a() {}\n";
const MORE_ITEMS: &str = "pub fn more() {}\n";
fn stub(proc_macro: bool) -> String {
    nuke_stub("x", proc_macro, NUKE_MARKER)
}

/// A repository in DOC whose crate `x` has `lib.rs` and `more.rs` committed with items.
fn committed(tag: &str) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "clr-gut-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("crates/x/src")).unwrap();
    std::fs::create_dir_all(root.join("design_rounds")).unwrap();
    std::fs::write(
        root.join("design_rounds/202601010000_changelist.doc.md"),
        "x\n",
    )
    .unwrap();
    std::fs::write(root.join("crates/x/src/lib.rs"), LIB_ITEMS).unwrap();
    std::fs::write(root.join("crates/x/src/more.rs"), MORE_ITEMS).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.email", "t@example.com"],
        &["config", "user.name", "t"],
        &["add", "-A"],
        &["commit", "-q", "-m", "seed", "--no-gpg-sign"],
    ] {
        git(&root, args);
    }
    root
}

fn git(root: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git {args:?}");
}

/// The gate's findings over the repository as it stands.
fn gate(root: &Path) -> Vec<LintError> {
    let crates = std::collections::BTreeSet::new();
    let src_dirs = [root.join("crates")];
    crate::testkit::check_repo(&ChangelistRequired, crate::RepoInput {
        mock_dir:    root,
        repo_root:   root,
        all_crates:  &crates,
        src_dirs:    &src_dirs,
        invocation:  None,
        canon_paths: &[],
        open_panels: &[],
        ref_files:   &crate::src_layout::NO_REF_FILES,
        registry:    &Default::default(),
    })
}

/// The edit question over a fresh reading of `root`, as the write guard asks it.
fn admits(root: &Path, file: &str, text: &str) -> bool {
    let reading = crate::Reading::new(root);
    admits_edit(&reading.view(), root, &[root.join("crates")], file, text)
}

fn done(root: PathBuf) {
    let _ = std::fs::remove_dir_all(root);
}

/// The control: an item edit outside IMPL is refused.
#[test]
fn an_item_edit_is_refused() {
    let root = committed("item");
    std::fs::write(root.join("crates/x/src/more.rs"), "pub fn changed() {}\n").unwrap();
    let found = gate(&root);
    done(root);
    assert_eq!(found.len(), 1, "{found:?}");
}

/// A file that held items, gutted to a comment, is refused at the gate and the guard.
#[test]
fn a_file_gutted_to_a_comment_is_refused() {
    let root = committed("gut");
    let guard = admits(&root, "crates/x/src/more.rs", "// emptied\n");
    std::fs::write(root.join("crates/x/src/more.rs"), "// emptied\n").unwrap();
    let found = gate(&root);
    done(root);
    assert!(!guard, "the guard must refuse cutting items out");
    assert_eq!(found.len(), 1, "{found:?}");
}

/// A file that declared nothing at `HEAD` and still declares nothing is scaffolding.
#[test]
fn scaffolding_rewritten_as_scaffolding_passes() {
    let root = committed("scaffold");
    std::fs::write(root.join("crates/x/src/docs.rs"), "//! Docs.\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "docs", "--no-gpg-sign"]);
    let guard = admits(&root, "crates/x/src/docs.rs", "//! Other docs.\n");
    std::fs::write(root.join("crates/x/src/docs.rs"), "//! Other docs.\n").unwrap();
    let found = gate(&root);
    done(root);
    assert!(guard);
    assert!(found.is_empty(), "{found:?}");
}

/// A stub with the marker written over `lib.rs` while `more.rs` survives is no nuke.
#[test]
fn a_marker_stub_beside_surviving_source_is_refused() {
    let root = committed("stub");
    let guard = admits(&root, "crates/x/src/lib.rs", &stub(false));
    std::fs::write(root.join("crates/x/src/lib.rs"), &stub(false)).unwrap();
    let found = gate(&root);
    done(root);
    assert!(!guard, "the guard must refuse the stub");
    assert!(!found.is_empty(), "{found:?}");
}

/// A removal with no nuke is refused.
#[test]
fn a_removal_without_a_nuke_is_refused() {
    let root = committed("rm");
    git(&root, &["rm", "-q", "crates/x/src/more.rs"]);
    let found = gate(&root);
    done(root);
    assert_eq!(found.len(), 1, "{found:?}");
}

/// The whole shape of a nuke passes: the stub, and every other source file removed.
#[test]
fn a_whole_nuke_passes() {
    for proc_macro in [false, true] {
        let root = nuked("nuke", proc_macro);
        let found = gate(&root);
        done(root);
        assert!(found.is_empty(), "proc macro {proc_macro}: {found:?}");
    }
}

/// The stub is admitted at the guard once nothing else is left, for both kinds.
#[test]
fn the_stub_of_either_kind_is_admitted_once_nothing_else_is_left() {
    for proc_macro in [false, true] {
        let root = committed("stubguard");
        git(&root, &["rm", "-q", "crates/x/src/more.rs"]);
        let admitted = admits(&root, "crates/x/src/lib.rs", &stub(proc_macro));
        done(root);
        assert!(admitted, "proc macro {proc_macro}");
    }
}

/// `x` nuked: the stub staged over `lib.rs` and `more.rs` removed.
fn nuked(tag: &str, proc_macro: bool) -> PathBuf {
    let root = committed(tag);
    std::fs::write(root.join("crates/x/src/lib.rs"), stub(proc_macro)).unwrap();
    git(&root, &["rm", "-q", "crates/x/src/more.rs"]);
    git(&root, &["add", "-A"]);
    root
}

/// A nuke excuses removals and the stub, never new items in the crate.
#[test]
fn items_written_into_a_nuked_crate_are_refused_by_the_guard() {
    let root = nuked("guard", false);
    let over_lib = admits(&root, "crates/x/src/lib.rs", "pub fn back() {}\n");
    let beside = admits(&root, "crates/x/src/evil.rs", "pub fn evil() {}\n");
    let the_stub = admits(&root, "crates/x/src/lib.rs", &stub(false));
    done(root);
    assert!(!over_lib, "items over the stub");
    assert!(!beside, "items in a new file");
    assert!(the_stub, "the stub itself");
}

/// Code outside `src`, beside a nuke, is judged on its own text.
#[test]
fn code_outside_src_beside_a_nuke_is_refused() {
    let root = nuked("outside", false);
    std::fs::write(root.join("crates/x/build.rs"), "fn main() {}\n").unwrap();
    git(&root, &["add", "-A"]);
    let found = gate(&root);
    done(root);
    assert_eq!(found.len(), 1, "{found:?}");
}

/// Items written over a staged stub and left unstaged are refused.
#[test]
fn unstaged_items_over_a_nuke_stub_are_refused() {
    let root = nuked("unstaged", false);
    std::fs::write(root.join("crates/x/src/lib.rs"), "pub fn back() {}\n").unwrap();
    let found = gate(&root);
    done(root);
    assert!(!found.is_empty(), "{found:?}");
}

/// A custom inner attribute can expand to items, so it is not known to declare nothing.
#[test]
#[ignore = "mockspace_changelist_required_reads_a_custom_inner_attribute: every inner attribute passes"]
fn a_custom_inner_attribute_is_not_known_to_declare_nothing() {
    assert!(scaffold::declares_nothing("#![no_std]\n"));
    assert!(!scaffold::declares_nothing("#![generate_items]\n"));
}
