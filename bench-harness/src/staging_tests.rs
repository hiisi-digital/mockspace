//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

/// The published SHA-256 of `abc` (FIPS 180-2, appendix B.1), so a record
/// written here holds a digest nothing in this module computed.
const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

/// A fresh directory with `release/` in it, its links resolved so a path
/// built under it compares equal to what `of_executable` finds.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bench-staging-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(RELEASE)).unwrap();
    std::fs::canonicalize(dir).unwrap()
}

/// A staged build holding one driver whose bytes are `abc`, with `record`
/// as its checksum record.
fn write_stage(dir: &Path, record: &str) {
    std::fs::write(dir.join("release/driver"), b"abc").unwrap();
    std::fs::write(dir.join(CHECKSUMS), record).unwrap();
    std::fs::write(dir.join(BUILD_RECORD), "commit: test\n").unwrap();
}

fn staged(name: &str) -> (PathBuf, StagedBuild) {
    let dir = scratch(name);
    write_stage(&dir, &format!("{ABC}  release/driver\n"));
    let build = StagedBuild::of_executable(&dir.join("release/driver")).unwrap();
    (dir, build)
}

#[test]
fn a_driver_in_a_staged_build_is_found_and_verifies() {
    let (dir, build) = staged("found");
    assert_eq!(build.dir(), dir.as_path());
    assert_eq!(build.release(), dir.join(RELEASE));
    assert_eq!(build.build_record().unwrap(), "commit: test\n");
    build.verify().unwrap();
    std::fs::remove_dir_all(&dir).ok();
}

/// `sha256sum -b` marks a binary-mode line with `*` in place of the second
/// space, and PowerShell's `Get-FileHash` writes the digest in capitals; the
/// record is read either way.
#[test]
fn a_binary_mode_line_and_a_capital_digest_verify() {
    for record in [
        format!("{ABC} *release/driver\n"),
        format!("{}  release/driver\n", ABC.to_ascii_uppercase()),
    ] {
        let dir = scratch("spellings");
        write_stage(&dir, &record);
        let build = StagedBuild::of_executable(&dir.join("release/driver")).unwrap();
        assert!(build.verify().is_ok(), "{record:?}: {:?}", build.verify());
        std::fs::remove_dir_all(&dir).ok();
    }
}

/// The negative control: a staged file replaced after staging, which is what
/// a build in another worktree does to a shared target directory.
#[test]
fn a_file_replaced_after_staging_fails_verify() {
    let (dir, build) = staged("replaced");
    std::fs::write(dir.join("release/driver"), b"abd").unwrap();
    match build.verify() {
        Err(StagingError::Changed {
            files,
        }) => assert_eq!(files, ["release/driver"]),
        other => panic!("a replaced file verified: {other:?}"),
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_file_gone_after_staging_fails_verify() {
    let (dir, build) = staged("gone");
    std::fs::write(dir.join("release/arm.so"), b"abc").unwrap();
    std::fs::write(
        dir.join(CHECKSUMS),
        format!("{ABC}  release/driver\n{ABC}  release/arm.so\n"),
    )
    .unwrap();
    std::fs::remove_file(dir.join("release/arm.so")).unwrap();
    assert!(matches!(build.verify(), Err(StagingError::Changed { .. })));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_record_naming_no_file_verifies_nothing_and_is_refused() {
    let (dir, build) = staged("empty");
    std::fs::write(dir.join(CHECKSUMS), "\n").unwrap();
    assert!(matches!(build.verify(), Err(StagingError::Empty)));
    std::fs::remove_dir_all(&dir).ok();
}

/// A record that leaves the running driver out would pass a driver replaced
/// after staging, since only the arms it names are read.
#[test]
fn a_record_without_the_running_driver_is_refused() {
    let (dir, build) = staged("nodriver");
    std::fs::write(dir.join("release/arm.so"), b"abc").unwrap();
    std::fs::write(dir.join("release/driver"), b"tampered").unwrap();
    std::fs::write(dir.join(CHECKSUMS), format!("{ABC}  release/arm.so\n")).unwrap();
    match build.verify() {
        Err(StagingError::DriverNotRecorded {
            driver,
        }) => assert_eq!(driver, "release/driver"),
        other => panic!("a record without the driver verified: {other:?}"),
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// The record names the driver by its path, so a spelling of the same path
/// with a doubled separator still names it.
#[test]
fn the_driver_is_found_by_its_path_not_by_its_spelling() {
    let dir = scratch("spelling");
    write_stage(&dir, &format!("{ABC}  release//driver\n"));
    let build = StagedBuild::of_executable(&dir.join("release/driver")).unwrap();
    assert!(build.verify().is_ok(), "{:?}", build.verify());
    std::fs::remove_dir_all(&dir).ok();
}

/// A record written on Windows by a tool that separates with `\`, such as
/// PowerShell, names the driver as `release\driver.exe`.
#[cfg(windows)]
#[test]
fn a_record_separated_by_backslashes_names_the_driver() {
    let dir = scratch("backslash");
    write_stage(&dir, &format!("{ABC}  release\\driver\n"));
    let build = StagedBuild::of_executable(&dir.join("release/driver")).unwrap();
    assert!(build.verify().is_ok(), "{:?}", build.verify());
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
        let (dir, build) = staged("malformed");
        std::fs::write(dir.join(CHECKSUMS), format!("{line}\n")).unwrap();
        assert!(
            matches!(build.verify(), Err(StagingError::Malformed { .. })),
            "{line:?} was read as a checksum line"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}

/// A record line naming a file outside the staged build would let the record
/// vouch for what the shared target directory holds, the files staging exists
/// to keep out. The file it names exists and has the recorded digest.
#[test]
fn a_record_line_reaching_outside_the_staged_build_is_refused() {
    let (dir, build) = staged("outside");
    let outside = dir
        .parent()
        .unwrap()
        .join(format!("outside-arm-{}", std::process::id()));
    std::fs::write(&outside, b"abc").unwrap();
    let lines = [
        format!(
            "{ABC}  ../{}",
            outside.file_name().unwrap().to_str().unwrap()
        ),
        format!("{ABC}  {}", outside.display()),
        format!("{ABC}  release/../release/driver"),
        format!("{ABC}  ./release/driver"),
    ];
    for line in lines {
        std::fs::write(
            dir.join(CHECKSUMS),
            format!("{ABC}  release/driver\n{line}\n"),
        )
        .unwrap();
        assert!(
            matches!(build.verify(), Err(StagingError::Malformed { .. })),
            "{line:?} was followed out of the staged build"
        );
    }
    std::fs::remove_file(&outside).ok();
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_driver_in_the_target_directory_is_refused() {
    let dir = scratch("target");
    std::fs::write(dir.join("release/driver"), b"unstaged").unwrap();
    let err = StagedBuild::of_executable(&dir.join("release/driver")).unwrap_err();
    assert!(matches!(err, StagingError::NotStaged { .. }), "{err}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_driver_outside_a_release_directory_is_refused() {
    let dir = scratch("deps");
    write_stage(&dir, &format!("{ABC}  release/driver\n"));
    std::fs::create_dir_all(dir.join("release/deps")).unwrap();
    std::fs::write(dir.join("release/deps/driver"), b"abc").unwrap();
    let err = StagedBuild::of_executable(&dir.join("release/deps/driver")).unwrap_err();
    assert!(matches!(err, StagingError::NotStaged { .. }), "{err}");
    std::fs::remove_dir_all(&dir).ok();
}

/// The link is made, kept as the same link when it already points here,
/// replaced when it points at an earlier run's build, removed when the run
/// lets go of it, and a directory or a file at its path is left alone. Run on
/// the unix call only: the Windows call needs a privilege a test machine may
/// not hold.
#[cfg(stage_link = "unix")]
#[test]
fn a_link_is_made_kept_replaced_when_stale_removed_after_and_never_over_a_non_link() {
    use std::os::unix::fs::MetadataExt;

    let (dir, build) = staged("link");
    let link = dir.join("target");
    let held = build.link_into(&link).unwrap();
    assert_eq!(std::fs::read_link(&link).unwrap(), dir);
    let first = link.symlink_metadata().unwrap().ino();
    let again = build.link_into(&link).unwrap();
    assert_eq!(
        link.symlink_metadata().unwrap().ino(),
        first,
        "a current link is kept, not remade"
    );
    drop(again);
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        dir,
        "a run that found the link already there removed it under the run that made it"
    );
    drop(held);
    assert!(
        link.symlink_metadata().is_err(),
        "the link outlived the run that held it"
    );

    std::os::unix::fs::symlink(std::env::temp_dir(), &link).unwrap();
    let held = build.link_into(&link).unwrap();
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        dir,
        "a stale link is replaced"
    );
    drop(held);

    std::fs::create_dir(&link).unwrap();
    assert!(matches!(
        build.link_into(&link),
        Err(StagingError::NotALink { .. })
    ));
    assert!(link.is_dir(), "a real directory is never removed");
    std::fs::remove_dir(&link).unwrap();

    std::fs::write(&link, b"somebody's").unwrap();
    assert!(matches!(
        build.link_into(&link),
        Err(StagingError::NotALink { .. })
    ));
    assert_eq!(
        std::fs::read(&link).unwrap(),
        b"somebody's",
        "a file is never removed"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// A driver started through a member's `target` link can be reported at the
/// link's path. The build found from there is the one the link points at, and
/// linking it again keeps the link rather than pointing it at itself.
#[cfg(stage_link = "unix")]
#[test]
fn a_driver_started_through_the_link_finds_the_staged_build_behind_it() {
    let (dir, build) = staged("through");
    let member = scratch("through-member");
    let link = member.join("target");
    let held = build.link_into(&link).unwrap();

    let through = StagedBuild::of_executable(&link.join("release/driver")).unwrap();
    assert_eq!(through.dir(), dir.as_path());
    let again = through.link_into(&link).unwrap();
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        dir,
        "the link points at itself"
    );
    assert_eq!(std::fs::read(link.join("release/driver")).unwrap(), b"abc");
    drop(again);
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        dir,
        "the second run removed the link the first still holds"
    );
    drop(held);
    std::fs::remove_dir_all(&member).ok();
    std::fs::remove_dir_all(&dir).ok();
}
