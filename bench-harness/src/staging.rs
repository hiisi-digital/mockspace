//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Where a bench driver's binaries were staged, and whether they still are.
//!
//! Worktrees of one repository usually build into one shared target directory,
//! so a driver timing arms out of it can load arms another worktree rebuilt a
//! minute ago, from another branch, and nothing fails: the arms still load, the
//! bridge hash still matches, and the result names a build that no longer
//! exists. A driver that times anything runs from a staged build instead, a
//! directory of its own that no build writes into.
//!
//! A staged build is `<dir>/release/`, holding the driver and every arm it
//! loads, the same shape as the target directory so the driver finds its arms
//! beside itself. Beside `release/` are two records: `binaries.sha256`, one line
//! per staged file in the format `sha256sum` writes (the hex digest, two spaces,
//! the path relative to `<dir>` with no `..`), and `build.txt`, naming the
//! commit the binaries were built from and whatever was dirty against it. The
//! consumer's staging step writes it; this module reads it.
//!
//! [`StagedBuild::of_current_exe`] refuses a driver that was not started from a
//! staged build, and [`StagedBuild::verify`] checks that every recorded file,
//! the running driver among them, still has the digest written at staging.
//! `StagedBuild::link_into`, behind the `stage-link` feature, points a member's
//! `target` at the staged build for as long as the returned [`StageLink`] is
//! held, for a manifest that names its arms under `target/`. A link is a
//! different call on each platform, so it is compiled for the targets
//! `build.rs` names and for no other. The lane that builds it:
//! `cargo test -p mockspace-bench-harness --features stage-link`, and for the
//! windows call `cargo check -p mockspace-bench-harness --features stage-link
//! --target x86_64-pc-windows-msvc`.

use std::fmt;
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

/// The directory inside a staged build holding the driver and its arms.
pub const RELEASE: &str = "release";

/// The checksum record beside `release/`, in `sha256sum`'s format.
pub const CHECKSUMS: &str = "binaries.sha256";

/// The build record beside `release/`.
pub const BUILD_RECORD: &str = "build.txt";

/// Why a staged build was refused or failed its check.
#[derive(Debug)]
pub enum StagingError {
    /// The executable does not sit at `<dir>/release/<name>` with
    /// `<dir>/binaries.sha256` beside it.
    NotStaged {
        exe: PathBuf,
    },
    /// A file of the staged build could not be read, or the link could not
    /// be made.
    Io {
        path:   PathBuf,
        source: std::io::Error,
    },
    /// A line of the checksum record is not `<64 hex digits>  <path>` with a
    /// path inside the staged build.
    Malformed {
        line: String,
    },
    /// The checksum record names no file, so it would verify nothing.
    Empty,
    /// The checksum record does not name the running driver, so the one
    /// binary certain to be loaded would go unchecked.
    DriverNotRecorded {
        driver: String,
    },
    /// Files whose digest differs from the one written at staging, or that
    /// are gone.
    Changed {
        files: Vec<String>,
    },
    /// Something other than a link is at the link's path, somebody's build or
    /// file rather than a link a driver made, and it is left as it is.
    NotALink {
        link: PathBuf,
    },
}

impl fmt::Display for StagingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotStaged {
                exe,
            } => {
                write!(
                    f,
                    "`{}` was not started from a staged build, so the arms beside it are \
                     whatever the shared target directory holds now; stage the member and run \
                     the staged driver, `<stage>/{RELEASE}/{}`",
                    exe.display(),
                    exe.file_name()
                        .map(|n| n.to_string_lossy())
                        .unwrap_or_default()
                )
            },
            Self::Io {
                path,
                source,
            } => write!(f, "{}: {source}", path.display()),
            Self::Malformed {
                line,
            } => {
                write!(
                    f,
                    "{CHECKSUMS} has a line that is not `<sha256>  <path inside the stage>`: \
                     {line:?}"
                )
            },
            Self::Empty => write!(f, "{CHECKSUMS} names no file, so it would verify nothing"),
            Self::DriverNotRecorded {
                driver,
            } => {
                write!(
                    f,
                    "{CHECKSUMS} does not name the running driver, `{driver}`"
                )
            },
            Self::Changed {
                files,
            } => {
                write!(
                    f,
                    "a staged binary changed after staging: {}",
                    files.join(", ")
                )
            },
            Self::NotALink {
                link,
            } => {
                write!(
                    f,
                    "{} is not a link; the arms would be read from it rather than from this \
                     driver's staged build",
                    link.display()
                )
            },
        }
    }
}

impl std::error::Error for StagingError {}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> StagingError + '_ {
    move |source| {
        StagingError::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

/// A staged build: `<dir>/release/` and the two records beside it, found from
/// the driver that runs out of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedBuild {
    /// The staged build's root, with every link in the path resolved.
    dir:    PathBuf,
    /// The driver's path relative to `dir`, as the checksum record names it.
    driver: String,
}

impl StagedBuild {
    /// The staged build the running driver was started from.
    pub fn of_current_exe() -> Result<Self, StagingError> {
        let exe = std::env::current_exe().map_err(io(Path::new("current_exe")))?;
        Self::of_executable(&exe)
    }

    /// The staged build an executable at `exe` belongs to: `exe`, its links
    /// resolved, sits at `<dir>/release/<name>` and `<dir>/binaries.sha256`
    /// is a file, which is what a staged build has and a target directory
    /// never does.
    ///
    /// The links are resolved because a driver started through a member's
    /// `target` link may be reported at the link's path on some platforms,
    /// and a build found there would be the link itself.
    pub fn of_executable(exe: &Path) -> Result<Self, StagingError> {
        let not_staged = || {
            StagingError::NotStaged {
                exe: exe.to_path_buf(),
            }
        };
        let real = std::fs::canonicalize(exe).map_err(|_| not_staged())?;
        let name = real
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(not_staged)?;
        let dir = real
            .parent()
            .filter(|p| p.file_name().is_some_and(|n| n == RELEASE))
            .and_then(Path::parent)
            .filter(|dir| dir.join(CHECKSUMS).is_file())
            .ok_or_else(not_staged)?;
        Ok(Self {
            dir:    dir.to_path_buf(),
            driver: format!("{RELEASE}/{name}"),
        })
    }

    /// The staged build's root, holding `release/` and the two records.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where the staged executable and arms are.
    pub fn release(&self) -> PathBuf {
        self.dir.join(RELEASE)
    }

    /// The build record: the commit, the time and whatever was dirty.
    pub fn build_record(&self) -> Result<String, StagingError> {
        let path = self.dir.join(BUILD_RECORD);
        std::fs::read_to_string(&path).map_err(io(&path))
    }

    /// Check every file the checksum record names against the digest written
    /// at staging, and name every one that changed or is gone. The record has
    /// to name the running driver.
    pub fn verify(&self) -> Result<(), StagingError> {
        let path = self.dir.join(CHECKSUMS);
        let record = std::fs::read_to_string(&path).map_err(io(&path))?;
        let mut changed = Vec::new();
        let mut named = 0usize;
        let mut driver_named = false;
        for line in record.lines().filter(|l| !l.is_empty()) {
            let (want, file) = parse_line(line)?;
            named += 1;
            driver_named |= file == self.driver;
            let ok = std::fs::read(self.dir.join(file))
                .is_ok_and(|bytes| hex(&Sha256::digest(&bytes)).eq_ignore_ascii_case(want));
            if !ok {
                changed.push(file.to_string());
            }
        }
        if named == 0 {
            return Err(StagingError::Empty);
        }
        if !driver_named {
            return Err(StagingError::DriverNotRecorded {
                driver: self.driver.clone(),
            });
        }
        if changed.is_empty() {
            Ok(())
        } else {
            Err(StagingError::Changed {
                files: changed,
            })
        }
    }
}

/// One line of the checksum record: the digest, in either case, and a path
/// inside the staged build, after two spaces or after a space and the `*` that
/// marks binary mode.
fn parse_line(line: &str) -> Result<(&str, &str), StagingError> {
    let malformed = || {
        StagingError::Malformed {
            line: line.to_string(),
        }
    };
    let (digest, rest) = line.split_at_checked(64).ok_or_else(malformed)?;
    let file = rest
        .strip_prefix("  ")
        .or_else(|| rest.strip_prefix(" *"))
        .filter(|f| !f.is_empty())
        .ok_or_else(malformed)?;
    let inside = Path::new(file)
        .components()
        .all(|c| matches!(c, Component::Normal(_)));
    if !inside || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(malformed());
    }
    Ok((digest, file))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A member's `target` pointing at a staged build, removed when this is
/// dropped, so the next plain build of the member writes into a target
/// directory of its own again and never into the staged build. Held for the
/// whole run.
#[cfg(any(stage_link = "unix", stage_link = "windows"))]
#[derive(Debug)]
#[must_use = "the link is removed when this is dropped"]
pub struct StageLink {
    link: PathBuf,
    to:   PathBuf,
}

#[cfg(any(stage_link = "unix", stage_link = "windows"))]
impl Drop for StageLink {
    /// Removes the link if it still points at the staged build; a link
    /// something else has since replaced is left to whoever made it.
    fn drop(&mut self) {
        let ours = self
            .link
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
            && std::fs::read_link(&self.link).is_ok_and(|to| to == self.to);
        if ours {
            let _ = link_os::remove(&self.link);
        }
    }
}

/// Built where the consumer turned on `stage-link` and the target is one
/// `build.rs` names; anywhere else there is no `link_into`, so a driver asking
/// for one does not build.
#[cfg(any(stage_link = "unix", stage_link = "windows"))]
impl StagedBuild {
    /// Point the link at `link` (a member's `./target`) at this staged build
    /// until the returned [`StageLink`] is dropped, so a manifest naming arms
    /// under `target/` reads the ones staged beside the driver.
    ///
    /// A link left by an earlier run points at that run's build, which is the
    /// stale set of arms staging exists to keep out, so a link pointing
    /// anywhere else is replaced, and one already pointing here is kept as it
    /// is. Anything that is not a link, a directory or a file, is refused
    /// rather than removed: it is somebody's, not a link a driver made.
    pub fn link_into(&self, link: &Path) -> Result<StageLink, StagingError> {
        let held = StageLink {
            link: link.to_path_buf(),
            to:   self.dir.clone(),
        };
        match link.symlink_metadata() {
            Ok(meta) if meta.file_type().is_symlink() => {
                if std::fs::read_link(link).is_ok_and(|to| to == self.dir) {
                    return Ok(held);
                }
                link_os::remove(link).map_err(io(link))?;
            },
            Ok(_) => {
                return Err(StagingError::NotALink {
                    link: link.to_path_buf(),
                });
            },
            Err(_) => {},
        }
        link_os::make(&self.dir, link).map_err(io(link))?;
        Ok(held)
    }
}

#[cfg(stage_link = "unix")]
mod link_os {
    use std::path::Path;

    pub(super) fn make(to: &Path, link: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(to, link)
    }

    pub(super) fn remove(link: &Path) -> std::io::Result<()> {
        std::fs::remove_file(link)
    }
}

/// A directory link on Windows is a directory symlink, which needs the
/// symlink privilege or developer mode; without either the call fails and its
/// error says so. It is removed as a directory.
#[cfg(stage_link = "windows")]
mod link_os {
    use std::path::Path;

    pub(super) fn make(to: &Path, link: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_dir(to, link)
    }

    pub(super) fn remove(link: &Path) -> std::io::Result<()> {
        std::fs::remove_dir(link)
    }
}

#[cfg(test)]
#[path = "staging_tests.rs"]
mod tests;
