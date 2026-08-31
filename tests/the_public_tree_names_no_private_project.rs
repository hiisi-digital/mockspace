//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Nothing a stranger reads names a project they cannot see.
//!
//! A tool developed against a handful of real consumers accretes their names:
//! in a doc comment explaining why a branch exists, in a test fixture, in an
//! error message telling somebody to set an environment variable to a path on
//! the author's own machine. Each one is harmless in isolation and reads as
//! sloppiness in aggregate, and a reader cannot tell which of them is load
//! bearing.
//!
//! Fixed once and greppable afterwards, because the class comes back one
//! comment at a time and nothing else notices.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Names that must not appear anywhere a reader of this repository reaches.
///
/// The consumers a reader cannot go and look at. Not an exhaustive list of
/// everything that should not be here, and it does not need to be: it is the
/// set that was actually found, so the class cannot come back the way it
/// arrived.
///
/// **Membership is decided by repository visibility, and it was measured rather
/// than assumed.** The list carried `arvo`, `hilavitkutin`, `vehje` and `notko`
/// for a long time and all four are public, so naming them is an ordinary
/// reference to a crate a reader can open. That mattered beyond tidiness: with
/// five public names in here the check produced mostly false positives over the
/// generated `docs/` tree, and the response was to exclude `docs/` from the
/// walk entirely. **So an over-broad list did not make the check stricter, it
/// got the check switched off over a whole tier one surface**, and the two real
/// leaks in there went unreported for as long as the exclusion stood.
///
/// A name joins this list when `gh repo view <owner>/<name> --json visibility`
/// says `PRIVATE`, and leaves it when that changes.
///
/// **Hand-maintained, which is the standing weakness.** Nothing re-runs that
/// command, so a sibling going private later stops being caught and nothing
/// says so. Deriving it would mean a network call from a test, which is worse;
/// what is written down instead is that the list is a snapshot of a fact that
/// can move.
const PRIVATE: &[&str] = &["clause-dev", "kolli", "ikiuni", "loisto", "loimu", "saalis"];

/// Where a path on somebody's own machine gives itself away.
///
/// A home directory rather than `/Users` alone, because the bare prefix also
/// matches prose about what a machine-specific link would look like, and that
/// prose is worth keeping.
const SOMEBODY_S_MACHINE: &[&str] = &["~/Dev/", "/Users/orgrinrt", "/home/orgrinrt"];

/// The two files that hold these names on purpose, because holding them is
/// what they do.
///
/// This one carries the list. `render_agent/tests.rs` carries a longer one and
/// fails the build when a generated agent instruction names a workspace, an
/// operator or a trunk, which is the same class caught one layer earlier.
fn is_a_guard(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    name == "the_public_tree_names_no_private_project.rs"
        || path.ends_with("src/render_agent/tests.rs")
}

fn tracked_files() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files"])
        .output()
        .expect("git could not be run");
    assert!(out.status.success(), "git ls-files failed");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        // `mock/` is this repository's own design trail and the v2 redesign,
        // which is history and work in progress rather than a surface.
        //
        // **It is excluded on purpose and it is not clean.** Twenty tracked
        // files under it carry `~/Dev/clause-dev` paths, in research notes and
        // in committed probe scripts where the path is what the probe reads.
        // Scrubbing them would rewrite the audit trail and break instruments
        // whose whole value is that they can be re-run.
        //
        // So this is a decision rather than an oversight, and the number is here
        // so a later reader can tell whether it grew. The exposure is a reader
        // learning the directory layout of one machine, which is real and is
        // smaller than the cost of editing evidence.
        //
        // `docs/` used to be excluded here, and the exclusion carried a comment
        // saying it should not be: it is a tier one surface a reader reaches
        // straight from the readme, and it was carrying private project names
        // and a home path the whole time the guard reported clean. Excluded now
        // only from the tarball, by `exclude` in the manifest, which is a
        // different question from what the landing page shows.
        //
        // A check that names the surface it is not checking is reporting on a
        // smaller population than its name claims, and nothing but the comment
        // said so.
        .filter(|l| !l.starts_with("mock/"))
        .map(|l| root.join(l))
        .collect()
}

#[test]
fn no_tracked_file_names_a_project_a_reader_cannot_see() {
    let mut found = Vec::new();
    for path in tracked_files() {
        if is_a_guard(&path) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            for name in PRIVATE {
                if line.contains(name) {
                    found.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "private project names in the public tree:\n{}",
        found.join("\n")
    );
}

#[test]
fn no_tracked_file_names_a_path_on_somebody_s_machine() {
    let mut found = Vec::new();
    for path in tracked_files() {
        if is_a_guard(&path) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            for needle in SOMEBODY_S_MACHINE {
                if line.contains(needle) {
                    found.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "a path on somebody's machine:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_checks_above_can_fail() {
    // The control. Both read every tracked file and both pass when the reader
    // finds nothing, so a `git ls-files` that returned an empty list, or a
    // filter that excluded everything, would report the tree clean.
    let files = tracked_files();
    assert!(
        files.len() > 50,
        "the reader found almost nothing: {}",
        files.len()
    );
    assert!(
        files
            .iter()
            .any(|p| p.extension().is_some_and(|e| e == "rs")),
        "the reader found no source at all"
    );
    // And that the needle would be seen if it were there. This file carries
    // every one of them and is skipped by name, so a skip that matched nothing
    // would fail here rather than silently passing the two above.
    let me = std::fs::read_to_string(file!())
        .or_else(|_| std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(file!())));
    if let Ok(text) = me {
        assert!(
            PRIVATE.iter().all(|n| text.contains(n)),
            "the list above does not contain what it says it contains"
        );
    }
}

/// The crates that reach crates.io, which is not this one.
///
/// `mockspace` is `publish = false` and has been since March, so a question
/// asked of its package list is a question about a tarball nobody receives.
/// What a stranger installs is `cargo-mock`, which is what the README's own
/// badges point at, and `mockspace-manifest` beside it.
///
/// Derived rather than written down, so a crate that starts publishing joins
/// the guard by doing that and not by somebody remembering to add a row here.
///
/// Derived from the workspace's own `members`, which is what makes that true.
/// A `read_dir` of the root reads neither the root's own manifest nor a member
/// nested below one directory, and this workspace has both: `.` is a member and
/// so is `benches/variants/multiply-xor`. So the crate the `exclude` comment
/// above names as the hazard, this one, flipping to `publish = true` in a line,
/// was the exact crate the scan could not see.
fn crates_that_publish() -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    for member in workspace_members(root) {
        let manifest = root.join(&member).join("Cargo.toml");
        if !manifest.is_file() {
            panic!("the workspace names `{member}` as a member and it has no Cargo.toml");
        }
        let text = std::fs::read_to_string(&manifest).expect("unreadable manifest");
        // Cargo's own rule: a crate publishes unless it says otherwise, so this
        // asks for the refusal rather than for the permission.
        //
        // Asking for `publish = true` was the same defect as the `read_dir` one
        // above, a rung further down. A member that simply drops the key starts
        // publishing and stops being scanned, and the floor below cannot see
        // that: it is a lower bound on the file count, and an unscanned crate
        // contributes nothing rather than subtracting. Measured before this
        // changed, with this package's own `publish = false` deleted: twelve
        // documents under `docs/` into a real tarball, suite green.
        if !text.lines().any(|l| l.trim() == "publish = false")
            && let Some(name) = package_name(&text)
        {
            out.push(name);
        }
    }
    out.sort();
    out
}

/// Every path in the root manifest's `members` array, `.` included.
///
/// Read out of the text rather than through a toml parser, matching the rest of
/// this file, which has no dependency to read one with. A glob member is
/// refused rather than skipped: expanding one is real work, and a member the
/// scan quietly drops is how this function was wrong before.
fn workspace_members(root: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(root.join("Cargo.toml")).expect("no root manifest");
    let (_, rest) = text
        .split_once("\nmembers = [")
        .expect("the root manifest names no workspace members");
    let (list, _) = rest
        .split_once(']')
        .expect("the members array does not close");
    list.split('"')
        // The quoted halves of `"a", "b"` are the odd indices.
        .skip(1)
        .step_by(2)
        .map(|m| {
            assert!(
                !m.contains('*'),
                "the workspace names the glob member `{m}`, which this scan cannot expand",
            );
            m.to_string()
        })
        .collect()
}

/// The `name = "..."` of the `[package]` table, which is the first one.
fn package_name(manifest: &str) -> Option<String> {
    let (_, rest) = manifest.split_once("\nname = \"")?;
    Some(rest.split_once('"')?.0.to_string())
}

/// Every file `cargo package` would put in the tarballs that actually ship.
///
/// A different question from `git ls-files`, and the one that decides what a
/// stranger sees.
fn files_that_would_ship() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let publishing = crates_that_publish();
    assert!(
        !publishing.is_empty(),
        "no crate here publishes, so every assertion below holds over an empty \
         set. Two did when this was written; losing both is the thing to notice."
    );

    let mut files = Vec::new();
    for name in &publishing {
        let out = Command::new(env!("CARGO"))
            .arg("package")
            .args(["--list", "--allow-dirty", "-p", name])
            .current_dir(root)
            .output()
            .expect("cargo could not be run");
        assert!(
            out.status.success(),
            "cargo package --list -p {name} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        // Paths are relative to the member's own directory, not to the repo
        // root, and `readme = "../README.md"` arrives as `README.md`. So the
        // member's own directory is tried first and the root only when that
        // finds nothing: taking both would read the root's `Cargo.toml`,
        // `LICENSE` and `src/lib.rs` as shipping, and those belong to a crate
        // that does not publish. It scanned five root files where one ships.
        let before = files.len();
        files.extend(
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter_map(|l| {
                    let member = root.join(name).join(l.trim());
                    let at_root = root.join(l.trim());
                    [member, at_root].into_iter().find(|p| p.is_file())
                }),
        );
        assert!(
            files.len() > before,
            "cargo listed a package for {name} and not one of its paths resolved \
             to a file, so this is scanning nothing for it"
        );
    }
    files.sort();
    files.dedup();
    files
}

#[test]
fn nothing_in_the_tarball_names_a_project_a_reader_cannot_see() {
    // Measured before this existed: the crate would have shipped four documents
    // carrying private project names and home paths, three of them under
    // `docs/research/`, which is bug notes for whoever works on this engine and
    // not for anybody consuming it.
    let shipped = files_that_would_ship();
    // A coarse backstop only. The real one is per crate, inside
    // `files_that_would_ship`, which refuses a crate whose every listed path
    // resolved to nothing; that catches a broken scan for one member, which a
    // total cannot. This number is low on purpose: it used to sit above a count
    // padded by root files that do not ship, so raising it again would be
    // pinning the noise.
    assert!(
        shipped.len() > 5,
        "only {} files listed, so this is measuring the wrong thing",
        shipped.len()
    );
    assert!(
        shipped.iter().any(|p| p.ends_with("README.md")),
        "the readme does not ship, and it is the one file a stranger certainly \
         reads: {shipped:?}"
    );

    let mut found = Vec::new();
    for path in shipped {
        if is_a_guard(&path) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for needle in PRIVATE.iter().chain(SOMEBODY_S_MACHINE) {
            if text.contains(needle) {
                found.push(format!("{}: {needle}", path.display()));
            }
        }
    }

    assert!(
        found.is_empty(),
        "these would ship to the registry naming something a reader cannot see:\n{}",
        found.join("\n"),
    );
}

/// The readme is the landing page on the registry, where a relative link resolves
/// against nothing. Every path it points at has to be in the tarball beside it,
/// or be an absolute URL.
///
/// Found by excluding `docs/` from the package: three links in the readme kept
/// pointing into it and would have rendered as dead text on crates.io while
/// working perfectly on GitHub, which is the way round nobody notices.
#[test]
fn every_relative_link_in_the_readme_points_at_something_that_ships() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = std::fs::read_to_string(root.join("README.md")).expect("the readme");
    let shipped: Vec<String> = files_that_would_ship()
        .iter()
        .filter_map(|p| p.strip_prefix(root).ok())
        .map(|p| p.display().to_string())
        .collect();

    let mut dead = Vec::new();
    for (at, line) in readme.lines().enumerate() {
        // `[text](target)` and a bare backticked path, which is how this readme
        // pointed at the usage guide before the links were made absolute.
        for target in link_targets(line) {
            if target.starts_with("http") || target.starts_with('#') {
                continue;
            }
            let target = target.split('#').next().unwrap_or(&target).to_string();

            // A path that does not exist here is describing the reader's own
            // repository, not pointing into this one: `mock/lints/<name>.rs` and
            // `.git/hooks/` are things a consumer has, and both read as links to
            // a naive matcher. The failure this catches is narrower and is the
            // one that actually happened: a path that is real in this tree and
            // absent from the tarball, so the link works on GitHub and dies on
            // the registry, which is the way round nobody notices.
            if !root.join(&target).exists() {
                continue;
            }
            if !shipped.iter().any(|s| *s == target) {
                dead.push(format!("{}:{}: {target}", "README.md", at + 1));
            }
        }
    }

    assert!(
        dead.is_empty(),
        "these readme references resolve to nothing for a reader on the registry:\n{}",
        dead.join("\n"),
    );
}

/// Markdown link targets, `[text](target)`, and nothing else.
///
/// A backticked path is prose rather than a link and is deliberately not checked.
/// An earlier version did check them and produced three false positives in one
/// run, all of the same kind: `mock/lints/<name>.rs`, `.git/hooks/` and
/// `mock/agent/config.toml` are paths in the reader's own repository, and two of
/// them happen to exist in this one too, so no test of existence can tell them
/// apart from a reference. A link is the shape that actually breaks on the
/// registry, and it is the shape this reads.
fn link_targets(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '(' && i > 0 && chars[i - 1] == ']' {
            if let Some(end) = chars[i ..].iter().position(|c| *c == ')') {
                out.push(chars[i + 1 .. i + end].iter().collect());
                i += end;
            }
        }
        i += 1;
    }
    out.extend(backticked_paths(line));
    out
}

/// Backticked spans that name a file in this tree.
///
/// The shape that actually broke, and the one a markdown-link scan cannot see:
/// the readme pointed at the usage guide as `` `docs/USAGE_GUIDE.md` `` rather
/// than as a link, so excluding `docs/` left three dangling references that
/// every `](...)` matcher walked straight past.
///
/// A span has to carry a separator and an extension to count, so `` `cargo mock
/// lock` `` and `` `Header` `` are not read as paths.
///
/// `mock/` and `.git/` are dropped, and that is the part worth stating: both
/// exist in this repository and both name the **reader's** tree when the readme
/// mentions them, because a consumer has a `mock/` of their own and a
/// `.git/hooks/` of their own. The existence check the caller runs cannot tell
/// those apart from a real reference, since they are real here too.
fn backticked_paths(line: &str) -> Vec<String> {
    line.split('`')
        .skip(1)
        .step_by(2)
        .filter(|s| {
            s.contains('/')
                && s.contains('.')
                && !s.contains(' ')
                && !s.starts_with("http")
                && !s.starts_with('/')
                && !s.starts_with("mock/")
                && !s.starts_with(".git")
        })
        .map(str::to_string)
        .collect()
}

#[test]
fn the_reference_scan_finds_both_shapes_a_readme_points_with() {
    // The control for the test above, and it belongs here rather than there.
    //
    // Every reference in this readme is an absolute url today, so that test
    // legitimately reaches nothing, and a `reached > 0` assertion inside it
    // would fail on a readme that is correct. What has to be guarded is the
    // scanner: one that found nothing would report the readme clean forever,
    // and it did, because it only knew `](...)` while the three references that
    // actually broke were backticked prose.
    let found = link_targets(
        "see [the guide](docs/USAGE_GUIDE.md) and `docs/REFERENCE-SYNTAX.md` for more",
    );
    assert!(
        found.iter().any(|t| t == "docs/USAGE_GUIDE.md"),
        "the markdown-link shape is not found: {found:?}"
    );
    assert!(
        found.iter().any(|t| t == "docs/REFERENCE-SYNTAX.md"),
        "the backticked shape is not found, which is the one that broke: {found:?}"
    );

    // And what it must not read as a reference: a command, a type, a url, and
    // the two trees that belong to the reader rather than to this repository.
    let quiet = link_targets(
        "run `cargo mock lock`, see `Header`, at `https://e.invalid/x.md`, \
         edit `mock/agent/config.toml` or `.git/hooks/pre-commit`",
    );
    assert!(quiet.is_empty(), "read something as a reference: {quiet:?}");
}

#[test]
fn the_publish_scan_reaches_every_member_including_this_one() {
    // The control for `crates_that_publish`, and the defect it names is one it
    // could never report itself: a scan that cannot see a crate reports that
    // crate as not publishing, which is indistinguishable from the truth.
    //
    // Measured when this was a `read_dir` of the root: flipping this package to
    // `publish = true` and dropping the `exclude` line put twelve documents
    // under `docs/` into a real tarball, private names and home paths included,
    // and the suite stayed green. The root manifest was never opened.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let members = workspace_members(root);

    assert!(
        members.iter().any(|m| m == "."),
        "the root package is not in the scanned set, so it can never join the \
         guard by publishing: {members:?}"
    );
    assert!(
        members.iter().any(|m| m.matches('/').count() >= 2),
        "no member nested below one directory is in the scanned set, and this \
         workspace has several: {members:?}"
    );
    // Every member resolves, which is what lets `crates_that_publish` panic
    // rather than skip on one that does not.
    for m in &members {
        assert!(
            root.join(m).join("Cargo.toml").is_file(),
            "the workspace names `{m}` and it has no manifest"
        );
    }

    // And the parse itself, on text rather than on this repository, so the two
    // assertions above cannot both hold by accident of one hand-written array.
    let d = tempfile::tempdir().expect("no tempdir");
    std::fs::write(
        d.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\n    \".\",\n    \"a/b/c\",\n]\n\n[package]\nname = \"x\"\n",
    )
    .expect("unwritable");
    assert_eq!(workspace_members(d.path()), vec![".", "a/b/c"]);
}
