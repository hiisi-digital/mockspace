//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Where the engine puts generated build output, and why it is invisible to
//! the desktop indexer.
//!
//! Everything the engine generates goes under a `target/` directory: the lint
//! and tool cdylib crate, the hooks, the bench scaffolding, the registry
//! schemas. All derived, all churning on every run, none of it source anybody
//! searches for.
//!
//! Spotlight does not know that. Left unmarked, a build tree is a few thousand
//! new files per run for `mdworker` to open, read and index, and it costs real
//! machine while contributing nothing. Measured on one probe: two throwaway
//! build trees under `/private/tmp` put four `mdworker_shared` processes and
//! `mds` on the CPU at once, and `mds_stores` was still at 180% cleaning up
//! after they were deleted.
//!
//! A `.metadata_never_index` file in a directory takes that directory and
//! everything under it out of the index. Writing it is the engine's job rather
//! than the consumer's: a project should not have to know the desktop indexer
//! exists to avoid paying for it.
//!
//! **Asking where a thing goes and making it go there are separate.** [`target_dir`]
//! is a pure path computation and creates nothing. [`ensure`] creates and marks,
//! and is called only where a write actually follows. The split is not tidiness:
//! `bootstrap::activate` computes the hooks directory in order to test
//! `.exists()` and refuse, and a precondition check that creates the thing it is
//! checking for is a bug that reports success.

use std::fs;
use std::path::{Path, PathBuf};

/// The marker macOS reads. Harmless everywhere else: an empty dotfile.
const MARKER: &str = ".metadata_never_index";

/// `<root>/target`. Pure: creates nothing, writes nothing.
pub fn target_dir(root: &Path) -> PathBuf {
    root.join("target")
}

/// The directory under `target/` the generated lint crate lives in. Every
/// site that generates, builds or addresses that crate names it through this,
/// so a move cannot leave one of them behind.
pub const LINT_CRATE: &str = "mockspace-lints";

/// Where the generated lint crate sits for the mock directory `root`:
/// `<root>/target/mockspace-lints`. Pure: creates nothing, writes nothing.
pub fn lint_crate_dir(root: &Path) -> PathBuf {
    target_dir(root).join(LINT_CRATE)
}

/// `path` with its `.` and `..` components folded away, without touching the
/// disk: a pack's directory is resolved the way cargo reads the path, and a
/// symlink on the way is not followed by either side.
pub fn lexically_normal(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {},
            Component::ParentDir => {
                out.pop();
            },
            other => out.push(other),
        }
    }
    out
}

/// Create `dir` and take it out of the desktop index. Returns it, so a write
/// site reads as one expression.
///
/// NOTE: best effort throughout. an unwritable directory is a real failure and
/// the caller that needs it reports it far better than a panic here would.
pub fn ensure(dir: PathBuf) -> PathBuf {
    let _ = fs::create_dir_all(&dir);
    mark(&dir);
    dir
}

/// `<root>/target`, created and out of the index.
pub fn ensure_target_dir(root: &Path) -> PathBuf {
    ensure(target_dir(root))
}

/// `<root>/target/<parts...>`, created, with the **`target/` root** marked.
///
/// NOTE: the mark goes on `target/` rather than on the subdirectory, so it
/// covers every generator including ones added later that never think about
/// this. Marking only the subdir was the first version's defect and it is
/// narrower than it reads.
pub fn ensure_under_target(root: &Path, parts: &[&str]) -> PathBuf {
    let mut dir = ensure_target_dir(root);
    for p in parts {
        dir = dir.join(p);
    }
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Take one existing directory out of the index, if it is not already.
///
/// Separate from [`ensure`] because of the case that motivates it: cargo honours
/// `CARGO_TARGET_DIR`, so the tree it writes is frequently **not** under any
/// path this engine computed. The artifact's own location is the only thing
/// that knows where it went, and that is learned from cargo rather than guessed.
pub fn mark(dir: &Path) {
    if !dir.is_dir() {
        return;
    }
    let marker = dir.join(MARKER);
    if !marker.exists() {
        let _ = fs::write(&marker, b"");
    }
}

/// Given a built artifact at `<somewhere>/<profile>/<lib>`, mark `<somewhere>`.
///
/// That is cargo's own layout: the artifact sits one directory below the target
/// root, in the profile directory. Anything shallower would mark a directory the
/// engine does not own, so a path that does not have two ancestors is left alone.
pub fn mark_target_root_of(artifact: &Path) -> Option<PathBuf> {
    let root = artifact.parent()?.parent()?.to_path_buf();
    mark(&root);
    Some(root)
}

/// Every spelling that already fixes where cargo builds, as seen from `start`.
///
/// The two environment variables first, then `[build] target-dir` in a cargo
/// config file, which cargo reads from every `.cargo/` above the directory it
/// runs in and then from `CARGO_HOME`. Any one is enough to decline. The config
/// is read from `start` upward, the mock directory, because every tree this
/// engine builds sits under it; a config further down, inside one tool, would
/// apply to that tool alone and is not looked for. Missing the config was not
/// harmless: a workspace whose root config puts every build in one `target/`
/// had each worktree build its own bench and test trees beside it, several
/// gigabytes a worktree.
pub fn build_target_dir(start: &Path) -> Option<std::ffi::OsString> {
    inherited_target_dir().or_else(|| {
        configured_target_dir(start, cargo_home().as_deref()).map(PathBuf::into_os_string)
    })
}

/// The two environment spellings alone. The lint store reads its answer as a path
/// against the repository root, which a config file's relative `target-dir` is
/// not, so it asks only these.
pub fn inherited_target_dir() -> Option<std::ffi::OsString> {
    // `CARGO_TARGET_DIR` first, which is cargo's own precedence over the
    // config-backed spelling. Only presence is read, so the order changes
    // nothing here and is kept honest for the next reader.
    std::env::var_os("CARGO_TARGET_DIR").or_else(|| std::env::var_os("CARGO_BUILD_TARGET_DIR"))
}

fn cargo_home() -> Option<PathBuf> {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))
}

/// `[build] target-dir` from the first cargo config that sets it, walking up
/// from `start` and then trying `cargo_home`, in cargo's own order. Both file
/// names count; where a directory holds both, the one without the extension
/// wins, which is what cargo's documentation says it does. A file that does not
/// parse is passed over, since cargo would refuse it and say why, which is a
/// better message than any this could give.
///
/// A relative value is resolved the way cargo resolves a path in a config file:
/// against the parent of the directory holding the file, so `target` in
/// `<repo>/.cargo/config.toml` is `<repo>/target`. The same holds for the file
/// in `cargo_home`, whose parent is then the base. An absolute value is
/// returned as it is.
pub fn configured_target_dir(start: &Path, cargo_home: Option<&Path>) -> Option<PathBuf> {
    let dirs = start
        .ancestors()
        .map(|d| d.join(".cargo"))
        .chain(cargo_home.map(Path::to_path_buf));
    for dir in dirs {
        for name in ["config", "config.toml"] {
            let Ok(text) = fs::read_to_string(dir.join(name)) else { continue };
            let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else { continue };
            if let Some(v) = doc
                .get("build")
                .and_then(|b| b.get("target-dir"))
                .and_then(|t| t.as_str())
            {
                let base = dir.parent().unwrap_or(&dir);
                return Some(base.join(v));
            }
        }
    }
    None
}

/// Whether this run may pass a `--target-dir` of its own.
///
/// Split out from `shared_target_dir` in `entry/test.rs` with what the
/// environment or a cargo config file set passed in rather than read, so the
/// decision is testable without a test mutating the process environment out
/// from under every other arm running beside it.
pub fn may_pass_target_dir(args: &[&str], inherited: Option<&std::ffi::OsStr>) -> bool {
    // An inherited target directory already puts every build in one place, and
    // `--target-dir` overrides it, so passing ours would open a second build
    // directory beside the one the machine chose and duplicate the exact thing
    // this is here to stop.
    if inherited.is_some() {
        return false;
    }
    // The caller's own flag wins, and only up to `--`, after which the tokens
    // belong to the test binary and say nothing about where cargo builds.
    !args
        .iter()
        .take_while(|a| **a != "--")
        .any(|a| *a == "--target-dir" || a.starts_with("--target-dir="))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asking_where_a_thing_goes_creates_nothing() {
        // the control the first version of this file did not have, and the
        // defect it did not have it for: `bootstrap::activate` computes the
        // hooks dir to test `.exists()` and refuse. a query that creates the
        // directory makes that check pass on a tree that has never been built.
        let tmp = tempfile::tempdir().unwrap();
        let dir = target_dir(tmp.path());
        assert_eq!(dir, tmp.path().join("target"));
        assert!(!dir.exists());
        assert!(!dir.join(MARKER).exists());
    }

    #[test]
    fn ensuring_creates_and_marks() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = ensure_target_dir(tmp.path());
        assert!(dir.is_dir());
        assert!(dir.join(MARKER).is_file());
    }

    #[test]
    fn an_existing_marker_is_left_alone() {
        // a person may have put something in it, or another tool may have. we
        // are not the only writer of this filename on a machine.
        let tmp = tempfile::tempdir().unwrap();
        let dir = ensure_target_dir(tmp.path());
        fs::write(dir.join(MARKER), b"do not clobber").unwrap();
        ensure_target_dir(tmp.path());
        assert_eq!(
            fs::read_to_string(dir.join(MARKER)).unwrap(),
            "do not clobber"
        );
    }

    #[test]
    fn the_mark_goes_on_the_target_root_not_the_subdirectory() {
        // the control against marking only what was asked for: a generator
        // added later writes a sibling subdir and inherits nothing.
        let tmp = tempfile::tempdir().unwrap();
        let sub = ensure_under_target(tmp.path(), &["mockspace", "registry-schemas"]);
        assert!(sub.is_dir());
        assert!(target_dir(tmp.path()).join(MARKER).is_file());
    }

    #[test]
    fn the_lint_crate_dir_is_where_the_generators_write() {
        // the hook reads the pure path, the lint path creates the directory;
        // they have to be the same one.
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            lint_crate_dir(tmp.path()),
            ensure_under_target(tmp.path(), &[LINT_CRATE])
        );
    }

    #[test]
    fn ensuring_twice_leaves_the_tree_alone() {
        // control against a helper that churns the thing it exists to stop
        // churning.
        let tmp = tempfile::tempdir().unwrap();
        let dir = ensure_target_dir(tmp.path());
        fs::write(dir.join("artifact"), b"x").unwrap();
        ensure_target_dir(tmp.path());
        assert!(dir.join("artifact").is_file());
    }

    #[test]
    fn an_artifact_marks_the_target_root_cargo_actually_used() {
        // the case the marker missed entirely: under CARGO_TARGET_DIR the tree
        // cargo writes is nowhere this engine computed, so marking
        // `<mock>/target` marks an almost-empty directory and leaves the
        // multi-gigabyte one indexed.
        let tmp = tempfile::tempdir().unwrap();
        let rel = tmp.path().join("elsewhere").join("release");
        fs::create_dir_all(&rel).unwrap();
        let artifact = rel.join("libmockspace_lints_abc.dylib");
        fs::write(&artifact, b"").unwrap();

        let marked = mark_target_root_of(&artifact).unwrap();
        assert_eq!(marked, tmp.path().join("elsewhere"));
        assert!(marked.join(MARKER).is_file());
        // and not the profile directory, which is cargo's and not a root
        assert!(!rel.join(MARKER).exists());
    }

    #[test]
    fn a_path_with_no_room_above_it_is_left_alone() {
        // control: no ancestors means no directory we can claim to own, and
        // marking one anyway would put the file somewhere arbitrary.
        assert!(mark_target_root_of(Path::new("lib.dylib")).is_none());
        // and marking a directory that does not exist writes nothing
        let tmp = tempfile::tempdir().unwrap();
        let absent = tmp.path().join("nope");
        mark(&absent);
        assert!(!absent.exists());
    }

    // `configured_target_dir`. Every tree is `<tmp>/work/deep`, with the configs
    // placed by the case, and the answers are compared as paths under `<tmp>`.

    fn put(dir: &Path, name: &str, text: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(name), text).unwrap();
    }

    fn deep(tmp: &Path) -> PathBuf {
        let d = tmp.join("work").join("deep");
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn no_config_anywhere_finds_nothing() {
        // control for every case below: a tree with no config and a cargo home
        // with none returns `None`, so a `Some` elsewhere came from a file.
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        assert_eq!(configured_target_dir(&deep(tmp.path()), Some(&home)), None);
        assert_eq!(configured_target_dir(&deep(tmp.path()), None), None);
    }

    #[test]
    fn the_deeper_cargo_dir_wins_over_an_ancestor() {
        let tmp = tempfile::tempdir().unwrap();
        let start = deep(tmp.path());
        put(&tmp.path().join("work/.cargo"), "config.toml", "[build]\ntarget-dir = \"/near\"\n");
        put(&tmp.path().join(".cargo"), "config.toml", "[build]\ntarget-dir = \"/far\"\n");
        assert_eq!(configured_target_dir(&start, None), Some(PathBuf::from("/near")));
    }

    #[test]
    fn an_ancestor_without_the_key_is_walked_past() {
        let tmp = tempfile::tempdir().unwrap();
        let start = deep(tmp.path());
        put(&tmp.path().join("work/.cargo"), "config.toml", "[net]\nretry = 1\n");
        put(&tmp.path().join(".cargo"), "config.toml", "[build]\ntarget-dir = \"/far\"\n");
        assert_eq!(configured_target_dir(&start, None), Some(PathBuf::from("/far")));
    }

    #[test]
    fn cargo_home_is_tried_last() {
        let tmp = tempfile::tempdir().unwrap();
        let start = deep(tmp.path());
        let home = tmp.path().join("home");
        put(&home, "config.toml", "[build]\ntarget-dir = \"/home-wide\"\n");
        assert_eq!(configured_target_dir(&start, Some(&home)), Some(PathBuf::from("/home-wide")));
        // and an ancestor's wins over it
        put(&tmp.path().join(".cargo"), "config.toml", "[build]\ntarget-dir = \"/tree\"\n");
        assert_eq!(configured_target_dir(&start, Some(&home)), Some(PathBuf::from("/tree")));
    }

    #[test]
    fn an_unparsable_file_is_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let start = deep(tmp.path());
        put(&tmp.path().join("work/.cargo"), "config.toml", "[build\ntarget-dir = = \"/bad\"\n");
        put(&tmp.path().join(".cargo"), "config.toml", "[build]\ntarget-dir = \"/good\"\n");
        assert_eq!(configured_target_dir(&start, None), Some(PathBuf::from("/good")));
    }

    #[test]
    fn a_dotted_key_is_read() {
        let tmp = tempfile::tempdir().unwrap();
        let start = deep(tmp.path());
        put(&tmp.path().join(".cargo"), "config.toml", "build.target-dir = \"/dotted\"\n");
        assert_eq!(configured_target_dir(&start, None), Some(PathBuf::from("/dotted")));
    }

    #[test]
    fn a_non_string_value_is_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let start = deep(tmp.path());
        put(&tmp.path().join("work/.cargo"), "config.toml", "[build]\ntarget-dir = 5\n");
        put(&tmp.path().join(".cargo"), "config.toml", "[build]\ntarget-dir = \"/after\"\n");
        assert_eq!(configured_target_dir(&start, None), Some(PathBuf::from("/after")));
    }

    #[test]
    fn the_file_without_an_extension_wins_when_both_exist() {
        // cargo's documentation: "If both files exist, Cargo will use the file
        // without the extension."
        let tmp = tempfile::tempdir().unwrap();
        let start = deep(tmp.path());
        let cargo = tmp.path().join(".cargo");
        put(&cargo, "config", "[build]\ntarget-dir = \"/plain\"\n");
        put(&cargo, "config.toml", "[build]\ntarget-dir = \"/toml\"\n");
        assert_eq!(configured_target_dir(&start, None), Some(PathBuf::from("/plain")));
    }

    #[test]
    fn a_relative_value_resolves_against_the_parent_of_the_cargo_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let start = deep(tmp.path());
        put(&tmp.path().join("work/.cargo"), "config.toml", "[build]\ntarget-dir = \"out/t\"\n");
        // not against `start`, and not against the `.cargo` directory itself
        assert_eq!(
            configured_target_dir(&start, None),
            Some(tmp.path().join("work").join("out/t"))
        );
    }

    #[test]
    fn a_relative_value_in_cargo_home_resolves_against_its_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let start = deep(tmp.path());
        let home = tmp.path().join("user").join(".cargo");
        put(&home, "config.toml", "[build]\ntarget-dir = \"shared\"\n");
        assert_eq!(
            configured_target_dir(&start, Some(&home)),
            Some(tmp.path().join("user").join("shared"))
        );
    }
}
