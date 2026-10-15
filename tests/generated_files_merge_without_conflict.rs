//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A conflict confined to generated files is not a conflict.
//!
//! Every scenario runs the real binary and real git in a scratch repository,
//! with the generated hooks active and `cargo mock` and `mock` shimmed to this
//! build, so what is asserted is what a consumer's clone does at `git merge`.
//!
//! The control carries the rest. Without the merge driver the same two
//! branches conflict on the generated document, which is what shows the
//! fixture makes a conflict at all: a no-conflict result over a fixture that
//! never conflicted would pass for any reason.
//!
//! What git does with a `pre-merge-commit` hook was measured first, on git
//! 2.43: the hook's staged change does not reach the merge commit. So a merge
//! that makes its own commit is followed up by a second commit (`post-merge`),
//! and one that stopped is regenerated at the commit that concludes it
//! (`pre-commit`, where the staged change does count). Both arms are below.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_mockspace");

struct Repo {
    _dir:  tempfile::TempDir,
    root:  PathBuf,
    home:  PathBuf,
    shims: PathBuf,
}

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

impl Repo {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let root = base.join("repo");
        let home = base.join("home");
        let shims = base.join("shims");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&home).unwrap();
        // `cargo mock` and `mock`, both this build. A real `cargo` is never
        // reached: nothing in these hooks asks for one.
        write(
            &shims.join("cargo"),
            "#!/bin/sh\n[ \"$1\" = mock ] && { shift; exec \"$MOCKSPACE_BIN\" --dir \"$(git rev-parse --show-toplevel)/mock\" \"$@\"; }\nexit 127\n",
        );
        write(
            &shims.join("mock"),
            "#!/bin/sh\nexec \"$MOCKSPACE_BIN\" --dir \"$(git rev-parse --show-toplevel)/mock\" \"$@\"\n",
        );
        for s in ["cargo", "mock"] {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(shims.join(s), fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        let repo = Repo {
            _dir: dir,
            root,
            home,
            shims,
        };
        repo.git(&["init", "-q", "-b", "dev"]);
        repo.git(&["config", "user.name", "t"]);
        repo.git(&["config", "user.email", "t@example.com"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo.git(&["config", "merge.ff", "false"]);
        write(
            &repo.root.join("mock/Cargo.toml"),
            "[workspace]\nmembers = []\nresolver = \"2\"\n",
        );
        write(
            &repo.root.join("mock/mockspace.toml"),
            "project_name = \"fixture\"\ncrate_prefix = \"fixture\"\n",
        );
        write(
            &repo.root.join("mock/ALPHA.md.tmpl"),
            "one\ntwo\nthree\nfour\nfive\n",
        );
        write(&repo.root.join("notes.txt"), "notes\n");
        repo.generate();
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-q", "--no-verify", "-m", "chore: base"]);
        repo
    }

    fn cmd(&self, program: &str) -> Command {
        let mut c = Command::new(program);
        let path = format!(
            "{}:{}",
            self.shims.display(),
            std::env::var("PATH").unwrap()
        );
        c.current_dir(&self.root)
            .env("PATH", path)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join("config"))
            .env("MOCKSPACE_BIN", BIN)
            .env_remove("MOCK_ROOT");
        c
    }

    fn git_out(&self, args: &[&str]) -> Output {
        self.cmd("git").args(args).output().unwrap()
    }

    fn git(&self, args: &[&str]) -> String {
        let out = self.git_out(args);
        assert!(
            out.status.success(),
            "git {args:?}: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    /// Generation alone, as a hook runs it, from inside the mock directory.
    fn generate(&self) {
        let out = self
            .cmd(BIN)
            .current_dir(self.root.join("mock"))
            .arg("--generate-only")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "generate: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn doc(&self) -> String {
        fs::read_to_string(self.root.join("docs/ALPHA.md")).unwrap()
    }

    fn edit_template(&self, from: &str, to: &str) {
        let path = self.root.join("mock/ALPHA.md.tmpl");
        let text = fs::read_to_string(&path).unwrap().replace(from, to);
        fs::write(path, text).unwrap();
    }

    /// Change the template, regenerate, commit. Waits out the second so the
    /// generation header differs between branches, which is what makes the
    /// documents conflict with no help from the body.
    fn side(&self, branch: &str, from: &str, to: &str) {
        self.edit_template(from, to);
        std::thread::sleep(std::time::Duration::from_millis(1100));
        self.generate();
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--no-verify", "-m", &format!("chore: edit on {branch}")]);
    }

    /// Two branches off the base, each changing a different template line, so the
    /// template merges cleanly and the generated document does not.
    fn two_sides(&self) {
        self.git(&["switch", "-q", "-c", "a"]);
        self.side("a", "one", "one, edited on a");
        self.git(&["switch", "-q", "dev"]);
        self.side("dev", "five", "five, edited on dev");
    }

    fn unmerged(&self) -> Vec<String> {
        self.git(&["diff", "--name-only", "--diff-filter=U"])
            .lines()
            .map(String::from)
            .collect()
    }

    fn status(&self) -> String {
        self.git(&["status", "--porcelain", "--", "docs", "mock", ".gitattributes", "notes.txt"])
    }

    fn subjects(&self) -> Vec<String> {
        self.git(&["log", "--format=%s"])
            .lines()
            .map(String::from)
            .collect()
    }
}

#[test]
fn control_without_the_driver_the_two_documents_conflict() {
    let r = Repo::new();
    r.two_sides();
    // The driver is registered in the clone's config and nowhere else, so
    // removing it is removing the thing under test.
    r.git(&["config", "--local", "--remove-section", "merge.mockspace-generated"]);
    let merged = r.git_out(&["merge", "a", "-m", "chore: merge a"]);
    assert!(
        !merged.status.success(),
        "the fixture must conflict without the driver: {}{}",
        String::from_utf8_lossy(&merged.stdout),
        String::from_utf8_lossy(&merged.stderr)
    );
    assert!(
        r.unmerged().contains(&"docs/ALPHA.md".to_string()),
        "{:?} {}{}",
        r.unmerged(),
        String::from_utf8_lossy(&merged.stdout),
        String::from_utf8_lossy(&merged.stderr)
    );
}

#[test]
fn documents_differing_on_both_sides_merge_with_no_conflict() {
    let r = Repo::new();
    r.two_sides();
    let merged = r.git_out(&["merge", "a", "-m", "chore: merge a"]);
    assert!(
        merged.status.success(),
        "{}{}",
        String::from_utf8_lossy(&merged.stdout),
        String::from_utf8_lossy(&merged.stderr)
    );
    assert!(r.unmerged().is_empty());

    // what generation gives on the merged tree, and the merged tree holds it
    let doc = r.doc();
    assert!(doc.contains("one, edited on a"), "{doc}");
    assert!(doc.contains("five, edited on dev"), "{doc}");
    r.generate();
    assert_eq!(r.doc(), doc, "regenerating the merged tree changes nothing");
    assert_eq!(
        r.status(),
        "",
        "the merged tree is committed, nothing is left staged or modified: {}{}",
        String::from_utf8_lossy(&merged.stdout),
        String::from_utf8_lossy(&merged.stderr)
    );

    // the merge commit, then the follow-up that regenerated, named for it
    let subjects = r.subjects();
    assert_eq!(
        subjects[0], "chore: regenerate generated files after merge",
        "{subjects:?}"
    );
    assert_eq!(subjects[1], "chore: merge a", "{subjects:?}");
    let in_head = r.git(&["show", "HEAD:docs/ALPHA.md"]);
    assert_eq!(
        in_head, doc,
        "the committed document is the regenerated one"
    );
}

#[test]
fn a_source_conflict_beside_them_stops_only_that_file() {
    let r = Repo::new();
    // the same template line on both sides: a real conflict, beside documents
    // that also differ
    r.git(&["switch", "-q", "-c", "a"]);
    r.side("a", "three", "three, on a");
    r.git(&["switch", "-q", "dev"]);
    r.side("dev", "three", "three, on dev");

    let merged = r.git_out(&["merge", "a", "-m", "chore: merge a"]);
    assert!(!merged.status.success(), "the template conflicts");
    assert_eq!(
        r.unmerged(),
        ["mock/ALPHA.md.tmpl"],
        "only the source file stops the merge"
    );

    // resolved by hand, committed through the real hooks: the document is
    // regenerated at the commit that concludes the merge
    write(
        &r.root.join("mock/ALPHA.md.tmpl"),
        "one\ntwo\nthree, resolved\nfour\nfive\n",
    );
    r.git(&["add", "mock/ALPHA.md.tmpl"]);
    let done = r.git_out(&["commit", "-q", "-m", "chore: merge a"]);
    assert!(
        done.status.success(),
        "{}{}",
        String::from_utf8_lossy(&done.stdout),
        String::from_utf8_lossy(&done.stderr)
    );
    let in_head = r.git(&["show", "HEAD:docs/ALPHA.md"]);
    assert!(in_head.contains("three, resolved"), "{in_head}");
    assert!(
        !in_head.contains("three, on dev") && !in_head.contains("three, on a"),
        "{in_head}"
    );
    r.generate();
    assert_eq!(
        r.git(&["show", "HEAD:docs/ALPHA.md"]),
        r.doc(),
        "HEAD holds the regenerated file"
    );
    assert_eq!(r.status(), "");
}

#[test]
fn a_merge_with_no_generated_change_is_left_alone() {
    let r = Repo::new();
    r.git(&["switch", "-q", "-c", "a"]);
    write(&r.root.join("notes.txt"), "notes, edited on a\n");
    r.git(&["commit", "-q", "--no-verify", "-am", "chore: edit notes"]);
    r.git(&["switch", "-q", "dev"]);
    write(&r.root.join("other.txt"), "other\n");
    r.git(&["add", "other.txt"]);
    r.git(&["commit", "-q", "--no-verify", "-m", "chore: add other"]);
    let before = r.subjects().len();

    let merged = r.git_out(&["merge", "a", "-m", "chore: merge a"]);
    assert!(merged.status.success());
    // exactly the two commits the merge brings: a's and the merge itself
    assert_eq!(r.subjects().len(), before + 2, "{:?}", r.subjects());
    assert_eq!(r.subjects()[0], "chore: merge a");
}

#[test]
fn aborting_a_stopped_merge_still_works() {
    let r = Repo::new();
    r.git(&["switch", "-q", "-c", "a"]);
    r.side("a", "three", "three, on a");
    r.git(&["switch", "-q", "dev"]);
    r.side("dev", "three", "three, on dev");
    let head = r.git(&["rev-parse", "HEAD"]);
    let merged = r.git_out(&["merge", "a", "-m", "chore: merge a"]);
    assert!(!merged.status.success());

    r.git(&["merge", "--abort"]);
    assert_eq!(r.git(&["rev-parse", "HEAD"]), head);
    assert!(r.unmerged().is_empty());
    assert_eq!(r.status(), "");
    assert!(!r.root.join(".git/MERGE_HEAD").exists());
}

/// The measurement the vehicle rests on, kept as a test so a git that changes
/// it says so. A `pre-merge-commit` hook that stages a change does not get it
/// into the merge commit, which is why a merge that makes its own commit is
/// followed up at `post-merge` rather than regenerated before the commit.
///
/// The arm that does carry a staged change is `pre-commit` at the commit that
/// concludes a stopped merge, and the scenario above that runs it is its test.
#[test]
fn git_drops_what_a_pre_merge_commit_hook_stages() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "t"]);
    git(&["config", "user.email", "t@example.com"]);
    git(&["config", "commit.gpgsign", "false"]);
    write(&root.join("gen.txt"), "base\n");
    write(&root.join("src.txt"), "base\n");
    write(&root.join("other.txt"), "base\n");
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "base"]);
    git(&["switch", "-q", "-c", "side"]);
    write(&root.join("other.txt"), "side\n");
    git(&["commit", "-qam", "side"]);
    git(&["switch", "-q", "main"]);
    write(&root.join("src.txt"), "main\n");
    git(&["commit", "-qam", "main"]);

    let hook = |name: &str| {
        let path = root.join(".git/hooks").join(name);
        write(
            &path,
            "#!/bin/sh\necho regenerated > gen.txt\ngit add gen.txt\n",
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
    };

    // a clean merge: the hook runs and its staged change is not in the commit
    hook("pre-merge-commit");
    git(&["merge", "-q", "-m", "merged", "side"]);
    assert_eq!(
        git(&["show", "HEAD:gen.txt"]),
        "base\n",
        "not carried into the merge commit"
    );
    assert_eq!(
        git(&["status", "--porcelain"]),
        "M  gen.txt\n",
        "left staged beside it"
    );
}
