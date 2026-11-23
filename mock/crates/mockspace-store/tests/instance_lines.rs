//! The lines that name an instance's files, in the exclude file and in the sparse-checkout file.

#[path = "../../../../tests/common/ambient.rs"]
mod ambient;
mod common;

use std::collections::BTreeSet;
use std::path::Path;

use common::{git, write};
use mockspace_store::{Held, exclude_lines, sparse_lines};

// Mutation: leave the directory line out of a pattern's lines; an excluded directory is never looked
// into, so a file below `notes/` that the pattern `notes/*` does not name would stay hidden.
#[test]
fn a_pattern_is_followed_by_the_line_that_takes_directories_back_out() {
    let held = Held::new(&["notes/*"]).unwrap();
    assert_eq!(exclude_lines("mock/research", Some(&held)), ["/mock/research/notes/*", "!/mock/research/notes/*/"]);
}

/// Every path of the world below, as `git` names it.
const PATHS: [&str; 12] = [
    "mock/crates/alpha/README.md.tmpl",
    "mock/crates/alpha/src/lib.rs",
    "mock/crates/alpha/docs/GUIDE.md.tmpl",
    "mock/crates/alpha/docs/keep.md",
    "mock/crates/beta/deep/er/x.md.tmpl",
    "mock/crates/ROOT.md.tmpl",
    "mock/research/notes/a.md",
    "mock/research/notes/sub/keep.md",
    "mock/research/top.md",
    "mock/research2/n.md",
    "mock/rounds/flat.md",
    "README.md",
];

/// The paths git reads as ignored with `lines` as the exclude file.
fn excluded(dir: &Path, lines: &[String]) -> BTreeSet<String> {
    std::fs::write(dir.join(".git/info/exclude"), lines.join("\n") + "\n").unwrap();
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["check-ignore", "--no-index", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.take().unwrap().write_all((PATHS.join("\n") + "\n").as_bytes())?;
            child.wait_with_output()
        })
        .unwrap();
    String::from_utf8_lossy(&out.stdout).lines().map(str::to_owned).collect()
}

/// The paths git leaves out of the work tree with `lines` as the managed part of the sparse file.
fn left_out(dir: &Path, lines: &[String]) -> BTreeSet<String> {
    let file = std::iter::once("/*".to_string()).chain(lines.iter().cloned()).collect::<Vec<_>>().join("\n") + "\n";
    git(dir, &["config", "core.sparseCheckout", "true"]);
    git(dir, &["config", "core.sparseCheckoutCone", "false"]);
    std::fs::write(dir.join(".git/info/sparse-checkout"), file).unwrap();
    git(dir, &["read-tree", "-mu", "HEAD"]);
    let listed = git(dir, &["ls-files", "-t"]);
    let out = listed.lines().filter_map(|l| l.strip_prefix("S ")).map(str::to_owned).collect();
    git(dir, &["config", "core.sparseCheckout", "false"]);
    git(dir, &["read-tree", "-mu", "HEAD"]);
    out
}

// Mutation: write the sparse lines by another route than the exclude lines, as the first version did,
// or toggle a negation the wrong way; git then hides from the branch a set of paths other than the
// one it keeps from writing over, and an instance file is either exposed or a branch file vanishes.
#[test]
fn git_hides_from_the_branch_exactly_the_paths_it_keeps_out_of_the_checkout() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q"]);
    for p in PATHS {
        write(dir, p, "x\n");
    }
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "all"]);
    let templates = Held::new(&["**/*.md.tmpl"]).unwrap();
    let notes = Held::new(&["notes/*"]).unwrap();
    for (rel, held) in [("mock/crates", Some(&templates)), ("mock/research", Some(&notes)), ("mock/research", None)] {
        let (e, s) = (exclude_lines(rel, held), sparse_lines(rel, held));
        let hidden = excluded(dir, &e);
        assert!(!hidden.is_empty(), "the control: {rel} {held:?} hides something, or this compares nothing");
        assert_eq!(hidden, left_out(dir, &s), "{rel} with {held:?}: exclude {e:?}, sparse {s:?}");
    }
}

// Mutation: leave the directory unescaped, so `[1]` is a class that matches `1` and a space or `#`
// ends or starts a pattern.
#[test]
fn a_directory_is_escaped_for_what_a_pattern_reads_as_syntax() {
    assert_eq!(exclude_lines("mock/odd[1]", None), ["/mock/odd\\[1\\]/"]);
    assert_eq!(exclude_lines("a b/#c/!d", None), ["/a\\ b/\\#c/\\!d/"]);
    assert_eq!(exclude_lines("a*b?c", None), ["/a\\*b\\?c/"]);
    assert_eq!(exclude_lines("", None), ["/"]);
}
