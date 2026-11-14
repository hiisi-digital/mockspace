//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

/// The published SHA-256 of `abc` (FIPS 180-2, appendix B.1), so a record
/// written here holds a digest nothing in this module computed.
const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bench-stage-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(RELEASE)).unwrap();
    dir
}

/// A stage holding one driver whose bytes are `abc`, recorded by the literal.
fn write_stage(dir: &Path, record: &str) {
    std::fs::write(dir.join("release/driver"), b"abc").unwrap();
    std::fs::write(dir.join(CHECKSUMS), record).unwrap();
    std::fs::write(dir.join(BUILD_RECORD), "commit: test\n").unwrap();
}

fn staged(name: &str) -> (PathBuf, Stage) {
    let dir = scratch(name);
    write_stage(&dir, &format!("{ABC}  release/driver\n"));
    let stage = Stage::of_executable(&dir.join("release/driver")).unwrap();
    (dir, stage)
}

#[test]
fn a_driver_in_a_stage_is_found_and_verifies() {
    let (dir, stage) = staged("found");
    assert_eq!(stage.dir(), dir.as_path());
    assert_eq!(stage.release(), dir.join(RELEASE));
    assert_eq!(stage.build_record().unwrap(), "commit: test\n");
    stage.verify().unwrap();
    std::fs::remove_dir_all(&dir).ok();
}

/// `sha256sum -b` marks a binary-mode line with `*` in place of the second
/// space, and the record is read either way.
#[test]
fn a_binary_mode_line_verifies() {
    let dir = scratch("binary");
    write_stage(&dir, &format!("{ABC} *release/driver\n"));
    Stage::of_executable(&dir.join("release/driver"))
        .unwrap()
        .verify()
        .unwrap();
    std::fs::remove_dir_all(&dir).ok();
}

/// The negative control: a staged file replaced after staging, which is what
/// a build in another worktree does to a shared target directory.
#[test]
fn a_file_replaced_after_staging_fails_verify() {
    let (dir, stage) = staged("replaced");
    std::fs::write(dir.join("release/driver"), b"abd").unwrap();
    match stage.verify() {
        Err(StageError::Changed {
            files,
        }) => assert_eq!(files, ["release/driver"]),
        other => panic!("a replaced file verified: {other:?}"),
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_file_gone_after_staging_fails_verify() {
    let (dir, stage) = staged("gone");
    std::fs::remove_file(dir.join("release/driver")).unwrap();
    assert!(matches!(stage.verify(), Err(StageError::Changed { .. })));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_record_naming_no_file_verifies_nothing_and_is_refused() {
    let (dir, stage) = staged("empty");
    std::fs::write(dir.join(CHECKSUMS), "\n").unwrap();
    assert!(matches!(stage.verify(), Err(StageError::Empty)));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_malformed_line_is_refused() {
    let short = &ABC[.. 63];
    let not_hex = format!("{}g", &ABC[.. 63]);
    let lines = [
        format!("{short}  release/driver"),
        format!("{not_hex}  release/driver"),
        format!("{ABC} release/driver"),
        format!("{ABC}  "),
        "release/driver".to_string(),
    ];
    for line in lines {
        let (dir, stage) = staged("malformed");
        std::fs::write(dir.join(CHECKSUMS), format!("{line}\n")).unwrap();
        assert!(
            matches!(stage.verify(), Err(StageError::Malformed { .. })),
            "{line:?} was read as a checksum line"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[test]
fn a_driver_in_the_target_directory_is_refused() {
    let dir = scratch("target");
    std::fs::write(dir.join("release/driver"), b"unstaged").unwrap();
    let err = Stage::of_executable(&dir.join("release/driver")).unwrap_err();
    assert!(matches!(err, StageError::NotStaged { .. }), "{err}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_driver_outside_a_release_directory_is_refused() {
    let dir = scratch("deps");
    write_stage(&dir, &format!("{ABC}  release/driver\n"));
    std::fs::create_dir_all(dir.join("release/deps")).unwrap();
    std::fs::write(dir.join("release/deps/driver"), b"abc").unwrap();
    let err = Stage::of_executable(&dir.join("release/deps/driver")).unwrap_err();
    assert!(matches!(err, StageError::NotStaged { .. }), "{err}");
    std::fs::remove_dir_all(&dir).ok();
}

/// The link is made, kept when it already points here, replaced when it
/// points at an earlier run's build, and a real directory is left alone.
/// Run on the unix call only: the Windows call needs a privilege a test
/// machine may not hold.
#[cfg(stage_link = "unix")]
#[test]
fn a_link_is_made_replaced_when_stale_and_a_directory_is_refused() {
    let (dir, stage) = staged("link");
    let link = dir.join("target");
    stage.link_into(&link).unwrap();
    assert_eq!(std::fs::read_link(&link).unwrap(), dir);
    stage.link_into(&link).unwrap();
    assert_eq!(std::fs::read_link(&link).unwrap(), dir);

    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(std::env::temp_dir(), &link).unwrap();
    stage.link_into(&link).unwrap();
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        dir,
        "a stale link is replaced"
    );

    std::fs::remove_file(&link).unwrap();
    std::fs::create_dir(&link).unwrap();
    assert!(matches!(
        stage.link_into(&link),
        Err(StageError::LinkOverDirectory { .. })
    ));
    assert!(link.is_dir(), "a real directory is never removed");
    std::fs::remove_dir_all(&dir).ok();
}
