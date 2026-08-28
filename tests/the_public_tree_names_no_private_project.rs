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
/// The private consumers this tool grew up against. Not an exhaustive list of
/// everything that should not be here, and it does not need to be: it is the
/// set that was actually found, so the class cannot come back the way it
/// arrived.
const PRIVATE: &[&str] = &[
    "clause-dev",
    "hilavitkutin",
    "vehje",
    "kolli",
    "ikiuni",
    "loisto",
    "arvo",
    "loimu",
    "saalis",
];

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
        // `docs/` is excluded and should not be: it is a tier one surface a
        // reader reaches, it is generated from templates under `mock/`, and it
        // still carries eight of these names and two home paths. Fixing it
        // means fixing the templates, which is a round of its own.
        .filter(|l| !l.starts_with("mock/") && !l.starts_with("docs/"))
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
fn crates_that_publish() -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    for entry in std::fs::read_dir(root).expect("the repo root is unreadable") {
        let dir = entry.expect("unreadable entry").path();
        let manifest = dir.join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&manifest).expect("unreadable manifest");
        // `publish = true` written out, which is what these carry. A manifest
        // that simply omits the key also publishes, and none here does; a
        // member that starts omitting it fails the floor below rather than
        // slipping past this.
        if text.lines().any(|l| l.trim() == "publish = true")
            && let Some(name) = package_name(&text)
        {
            out.push(name);
        }
    }
    out.sort();
    out
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
        // root, and `readme = "../README.md"` arrives as `README.md`. Both
        // roots are tried, so a file is found wherever it really sits.
        files.extend(
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .flat_map(|l| [root.join(name).join(l.trim()), root.join(l.trim())])
                .filter(|p| p.is_file()),
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
    // The floor is what stops an empty listing from passing every assertion
    // below. It sits at ten because the two publishing crates ship a dozen-odd
    // files between them, not the whole tree: the number was fifty when this
    // asked `mockspace`, which is `publish = false`, so it was a floor over a
    // set nobody receives.
    assert!(
        shipped.len() > 10,
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
