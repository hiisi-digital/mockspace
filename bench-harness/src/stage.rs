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
//! exists. A driver that times anything runs from a stage instead, a directory
//! of its own that nothing else builds into.
//!
//! A stage is `<dir>/release/`, holding the driver and every arm it loads, the
//! same shape as the target directory so the driver finds its arms beside
//! itself. Beside `release/` are two records: `binaries.sha256`, one line per
//! staged file in the format `sha256sum` writes (the hex digest, two spaces,
//! the path relative to `<dir>`), and `build.txt`, naming the commit the
//! binaries were built from and whatever was dirty against it. The consumer's
//! staging step writes the stage; this module reads it.
//!
//! [`Stage::of_current_exe`] refuses a driver that was not started from a
//! stage, and [`Stage::verify`] checks that every staged file still has the
//! digest written at staging. `Stage::link_into`, behind the `stage-link`
//! feature, points a member's `target` at the stage, for a manifest that names
//! its arms under `target/`. A link is a different call on each platform, so it
//! is compiled for the targets `build.rs` names and for no other.

use std::fmt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The directory inside a stage holding the driver and its arms.
pub const RELEASE: &str = "release";

/// The checksum record beside `release/`, in `sha256sum`'s format.
pub const CHECKSUMS: &str = "binaries.sha256";

/// The build record beside `release/`.
pub const BUILD_RECORD: &str = "build.txt";

/// Why a stage was refused or failed its check.
#[derive(Debug)]
pub enum StageError {
    /// The executable does not sit at `<dir>/release/<name>` with
    /// `<dir>/binaries.sha256` beside it.
    NotStaged {
        exe: PathBuf,
    },
    /// A file of the stage could not be read, or the link could not be made.
    Io {
        path:   PathBuf,
        source: std::io::Error,
    },
    /// A line of the checksum record is not `<64 hex digits>  <path>`.
    Malformed {
        line: String,
    },
    /// The checksum record names no file, so it would verify nothing.
    Empty,
    /// Files whose digest differs from the one written at staging, or that
    /// are gone.
    Changed {
        files: Vec<String>,
    },
    /// The link's path is a real directory, somebody's build rather than a
    /// link a driver made, and is left as it is.
    LinkOverDirectory {
        link: PathBuf,
    },
}

impl fmt::Display for StageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotStaged {
                exe,
            } => {
                write!(
                    f,
                    "`{}` was not started from a stage, so the arms beside it are whatever the \
                     shared target directory holds now; stage the member and run the staged \
                     driver, `<stage>/{RELEASE}/{}`",
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
                    "{CHECKSUMS} has a line that is not `<sha256>  <path>`: {line:?}"
                )
            },
            Self::Empty => write!(f, "{CHECKSUMS} names no file, so it would verify nothing"),
            Self::Changed {
                files,
            } => {
                write!(
                    f,
                    "a staged binary changed after staging: {}",
                    files.join(", ")
                )
            },
            Self::LinkOverDirectory {
                link,
            } => {
                write!(
                    f,
                    "{} is a directory, not a link; the arms would be read from it rather than \
                     from this binary's stage",
                    link.display()
                )
            },
        }
    }
}

impl std::error::Error for StageError {}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> StageError + '_ {
    move |source| {
        StageError::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

/// A staged build directory: `<dir>/release/` and the two records beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stage {
    dir: PathBuf,
}

impl Stage {
    /// The stage the running driver was started from.
    pub fn of_current_exe() -> Result<Self, StageError> {
        let exe = std::env::current_exe().map_err(io(Path::new("current_exe")))?;
        Self::of_executable(&exe)
    }

    /// The stage an executable at `exe` belongs to: `exe` sits at
    /// `<dir>/release/<name>` and `<dir>/binaries.sha256` is a file, which is
    /// what a stage has and a target directory never does.
    pub fn of_executable(exe: &Path) -> Result<Self, StageError> {
        let dir = exe
            .parent()
            .filter(|p| p.file_name().is_some_and(|n| n == RELEASE))
            .and_then(Path::parent);
        match dir {
            Some(dir) if dir.join(CHECKSUMS).is_file() => {
                Ok(Self {
                    dir: dir.to_path_buf(),
                })
            },
            _ => {
                Err(StageError::NotStaged {
                    exe: exe.to_path_buf(),
                })
            },
        }
    }

    /// The stage's root, holding `release/` and the two records.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where the staged executable and arms are.
    pub fn release(&self) -> PathBuf {
        self.dir.join(RELEASE)
    }

    /// The build record: the commit, the time and whatever was dirty.
    pub fn build_record(&self) -> Result<String, StageError> {
        let path = self.dir.join(BUILD_RECORD);
        std::fs::read_to_string(&path).map_err(io(&path))
    }

    /// Check every file the checksum record names against the digest written
    /// at staging, and name every one that changed or is gone.
    pub fn verify(&self) -> Result<(), StageError> {
        let path = self.dir.join(CHECKSUMS);
        let record = std::fs::read_to_string(&path).map_err(io(&path))?;
        let mut changed = Vec::new();
        let mut named = 0usize;
        for line in record.lines().filter(|l| !l.is_empty()) {
            let (want, file) = parse_line(line)?;
            named += 1;
            let ok = std::fs::read(self.dir.join(file))
                .is_ok_and(|bytes| hex(&Sha256::digest(&bytes)) == want);
            if !ok {
                changed.push(file.to_string());
            }
        }
        match (named, changed.is_empty()) {
            (0, _) => Err(StageError::Empty),
            (_, true) => Ok(()),
            (_, false) => {
                Err(StageError::Changed {
                    files: changed,
                })
            },
        }
    }
}

/// One line of the checksum record: the digest and the path, the path either
/// after two spaces or after a space and the `*` that marks binary mode.
fn parse_line(line: &str) -> Result<(&str, &str), StageError> {
    let malformed = || {
        StageError::Malformed {
            line: line.to_string(),
        }
    };
    let (digest, rest) = line.split_at_checked(64).ok_or_else(malformed)?;
    let file = rest
        .strip_prefix("  ")
        .or_else(|| rest.strip_prefix(" *"))
        .filter(|f| !f.is_empty())
        .ok_or_else(malformed)?;
    if !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(malformed());
    }
    Ok((digest, file))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Built where the consumer turned on `stage-link` and the target is one
/// `build.rs` names; anywhere else there is no `link_into`, so a driver asking
/// for one does not build.
#[cfg(any(stage_link = "unix", stage_link = "windows"))]
impl Stage {
    /// Point the link at `link` (a member's `./target`) at this stage, so a
    /// manifest naming arms under `target/` reads the ones staged beside the
    /// driver.
    ///
    /// A link left by an earlier run points at that run's build, which is the
    /// stale set of arms the stage exists to keep out, so a link pointing
    /// anywhere else is replaced. A real directory at `link` is refused rather
    /// than removed: it is somebody's build, not a link a driver made.
    pub fn link_into(&self, link: &Path) -> Result<(), StageError> {
        match link.symlink_metadata() {
            Ok(meta) if meta.file_type().is_symlink() => {
                if std::fs::read_link(link).is_ok_and(|to| to == self.dir) {
                    return Ok(());
                }
                link_os::remove(link).map_err(io(link))?;
            },
            Ok(_) => {
                return Err(StageError::LinkOverDirectory {
                    link: link.to_path_buf(),
                });
            },
            Err(_) => {},
        }
        link_os::make(&self.dir, link).map_err(io(link))
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
#[path = "stage_tests.rs"]
mod tests;
