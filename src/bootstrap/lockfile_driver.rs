//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A Cargo lockfile is settled from the merged manifests, never merged as text.
//!
//! Two branches that each add a workspace member add a package to the lock at the same spot, and
//! git alone stops on it. The lock is a function of the manifests, so whichever side a merge
//! takes it is wrong until cargo has resolved the merged tree. This wires the same driver script
//! the generated documents use (see [`super::driver_script`]) to the lockfiles, which keeps ours
//! and writes the path down where a regeneration will follow, and the regeneration that follows
//! ([`settle_lockfiles`]) asks cargo for the merged manifests' resolution, which rewrites the
//! lock with every member in it.
//!
//! **Which files**: the lock beside the mock directory's manifest when there is one, and every
//! path `mockspace.toml` names under `lockfiles`, as paths relative to the repository root.

use super::*;

/// The driver's name, as `.gitattributes` and the clone's config both spell it.
pub(crate) const LOCKFILE_DRIVER: &str = "mockspace-lockfile";

pub(crate) const LOCKFILE_NOTE: &str =
    "# mockspace: a lockfile is settled from the merged manifests, see the lockfile merge driver";

/// The lockfiles of this repository, repository-relative: the mock directory's own when it is
/// there, then the declared ones. Sorted and unique.
pub(crate) fn lockfile_paths(
    repo_root: &Path,
    mock_dir: &Path,
    declared: &[String],
) -> Vec<String> {
    let mut rels: Vec<String> = Vec::new();
    if mock_dir.join("Cargo.lock").is_file() {
        rels.extend(relative(repo_root, &mock_dir.join("Cargo.lock")));
    }
    for entry in declared {
        let rel = entry.trim_start_matches('/');
        if rel.ends_with("Cargo.lock")
            && !rel.split('/').any(|p| p == "..")
            && !rel.contains(char::is_whitespace)
        {
            rels.push(rel.to_string());
        } else {
            eprintln!(
                "warning: mockspace.toml `lockfiles` entry {entry:?} is not a repository-relative path \
                 to a Cargo.lock, so it governs nothing"
            );
        }
    }
    rels.sort();
    rels.dedup();
    rels
}

/// The attribute lines for `rels`, one per lockfile.
pub(crate) fn lockfile_attribute_lines(rels: &[String]) -> Vec<String> {
    rels.iter()
        .map(|r| format!("{} merge={LOCKFILE_DRIVER}", pattern(r)))
        .collect()
}

/// Register the driver in the given config. The command is the generated-file driver's, whose
/// script reads the lockfile's name and keeps ours where a regeneration follows.
pub(crate) fn register_lockfile_driver(target: &ConfigTarget, actions: &mut Vec<String>) {
    register_engine_driver(
        target,
        LOCKFILE_DRIVER,
        "mockspace lockfiles",
        &generated_driver_command(),
        actions,
    );
}

/// Resolve each lockfile again from the manifests beside it, so a lock kept from one side of a
/// merge has every member of the merged tree. Offline first, since a member is a path
/// dependency and needs no registry; online only if that cannot resolve. Returns the paths that
/// are now settled, or what cargo said.
pub(crate) fn settle_lockfiles(repo_root: &Path, rels: &[String]) -> Result<(), String> {
    for rel in rels {
        let Some(manifest) = repo_root
            .join(rel)
            .with_file_name("Cargo.toml")
            .canonicalize()
            .ok()
        else {
            continue;
        };
        let run = |offline: bool| {
            let mut c = Command::new("cargo");
            c.args(["metadata", "--format-version", "1", "--manifest-path"])
                .arg(&manifest);
            if offline {
                c.arg("--offline");
            }
            c.output()
        };
        let first = run(true).map_err(|e| format!("could not run cargo for {rel}: {e}"))?;
        if first.status.success() {
            continue;
        }
        let second = run(false).map_err(|e| format!("could not run cargo for {rel}: {e}"))?;
        if !second.status.success() {
            return Err(format!(
                "cargo could not resolve {rel}: {}",
                String::from_utf8_lossy(&second.stderr).trim()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mock_directorys_lock_and_the_declared_ones_are_named_and_a_bad_entry_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("mock")).unwrap();
        std::fs::write(root.join("mock/Cargo.lock"), "").unwrap();
        let declared = [
            "tools/x/Cargo.lock",
            "mock/Cargo.lock",
            "../Cargo.lock",
            "a b/Cargo.lock",
            "Cargo.toml",
        ]
        .map(String::from);
        let rels = lockfile_paths(root, &root.join("mock"), &declared);
        assert_eq!(rels, vec!["mock/Cargo.lock", "tools/x/Cargo.lock"]);
        assert_eq!(lockfile_attribute_lines(&rels), vec![
            format!("/mock/Cargo.lock merge={LOCKFILE_DRIVER}"),
            format!("/tools/x/Cargo.lock merge={LOCKFILE_DRIVER}"),
        ]);
    }

    #[test]
    fn a_repository_with_no_lock_names_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(lockfile_paths(dir.path(), &dir.path().join("mock"), &[]).is_empty());
    }
}

#[cfg(test)]
mod merge_tests {
    use mockspace_manifest::gate::MANAGED_MARKER;

    use super::*;

    fn git(root: &Path, args: &[&str]) -> std::process::Output {
        Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@e.c"])
            .args(args)
            .current_dir(root)
            .output()
            .unwrap()
    }

    fn git_ok(root: &Path, args: &[&str]) {
        let out = git(root, args);
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn member(root: &Path, name: &str) {
        let dir = root.join("crates").join(name);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )
        .unwrap();
        fs::write(dir.join("src/lib.rs"), "").unwrap();
    }

    /// A repository whose `dev` holds a workspace with the member `base` and its lock, and two
    /// branches that each add a member and settle the lock themselves, as a developer's own
    /// build would.
    fn two_branches() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git_ok(root, &["init", "-q", "-b", "dev"]);
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        member(root, "base");
        settle_lockfiles_at(root);
        git_ok(root, &["add", "."]);
        git_ok(root, &["commit", "-q", "-m", "base"]);
        for name in ["m1", "m2"] {
            git_ok(root, &["switch", "-q", "-c", name, "dev"]);
            member(root, name);
            settle_lockfiles_at(root);
            git_ok(root, &["add", "."]);
            git_ok(root, &["commit", "-q", "-m", name]);
        }
        git_ok(root, &["switch", "-q", "m1"]);
        dir
    }

    fn settle_lockfiles_at(root: &Path) {
        settle_lockfiles(root, &["Cargo.lock".to_string()]).unwrap();
    }

    fn lock(root: &Path) -> String {
        fs::read_to_string(root.join("Cargo.lock")).unwrap()
    }

    #[test]
    fn git_alone_stops_on_the_lock_of_two_branches_that_each_add_a_member() {
        // the control for the test below: this is the state it starts from
        let dir = two_branches();
        let out = git(dir.path(), &["merge", "--no-commit", "--no-ff", "m2"]);
        assert!(
            !out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert!(
            String::from_utf8_lossy(&out.stdout)
                .contains("CONFLICT (content): Merge conflict in Cargo.lock")
        );
        assert!(lock(dir.path()).contains("<<<<<<<"));
    }

    #[test]
    fn the_lockfile_driver_merges_clean_and_the_settled_lock_has_both_members() {
        let dir = two_branches();
        let root = dir.path();
        // the managed hook that would regenerate, answering the driver's probe; the tree has
        // no generated hook, which is the fresh worktree the driver used to refuse
        let hook = root.join(".git/hooks/post-merge");
        mockspace_manifest::write_executable(
            &hook,
            format!("#!/bin/sh\n{MANAGED_MARKER}\n[ -n \"$MOCKSPACE_PROBE\" ] && exit 0\nexit 9\n"),
        )
        .unwrap();
        let script = root.join(".git/mockspace/merge-generated");
        mockspace_manifest::write_executable(&script, super::super::driver_script::driver_script())
            .unwrap();
        let mut actions = Vec::new();
        register_lockfile_driver(&ConfigTarget::Clone(root), &mut actions);
        assert_eq!(actions, vec![format!(
            "registered the {LOCKFILE_DRIVER} merge driver"
        )]);
        fs::write(
            root.join(".gitattributes"),
            lockfile_attribute_lines(&["Cargo.lock".to_string()]).join("\n") + "\n",
        )
        .unwrap();
        git_ok(root, &["add", ".gitattributes"]);
        git_ok(root, &["commit", "-q", "-m", "attributes"]);

        let out = git(root, &["merge", "--no-commit", "--no-ff", "m2"]);
        assert!(
            out.status.success(),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let kept = fs::read_to_string(root.join(".git/mockspace-kept-ours")).unwrap();
        assert!(
            kept.lines().any(|l| l == "Cargo.lock"),
            "the driver wrote down that it kept ours: {kept}"
        );

        settle_lockfiles(root, &["Cargo.lock".to_string()]).unwrap();
        let settled = lock(root);
        for name in ["base", "m1", "m2"] {
            assert!(
                settled.contains(&format!("name = \"{name}\"")),
                "{name} is in the lock: {settled}"
            );
        }
        assert!(!settled.contains("<<<<<<<"));
    }
}
