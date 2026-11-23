//! Copies of the files a command is about to overwrite, change or remove, kept in the person's
//! cache for a day.
//!
//! Every write the engine makes over a file it did not make in the same breath goes through
//! [`keep`] first: a file of an instance, the sparse-checkout and exclude files, a hook, the git
//! config, a file a migration rewrites or renames. The copy is a last way back from a case nobody
//! foresaw, not the way the engine keeps an edit (the backup ref of an instance is), so it is plain
//! files under `<cache>/mockspace/kept/<seconds>-<pid>/<n>/<absolute path>`, one directory per run
//! and a number per copy, and a run takes out every directory of a run more than [`KEPT_FOR`]
//! seconds old. `<cache>` is `$XDG_CACHE_HOME`, or `$HOME/.cache`, and `$MOCKSPACE_KEPT_DIR`
//! names another directory instead, as every test binary does. A copy that cannot be made stops
//! the write: the caller reports it and writes nothing.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use mockspace_manifest::ambient::KEPT_DIR;

/// How long a run's copies are kept, in seconds: a day.
pub const KEPT_FOR: u64 = 24 * 60 * 60;

/// Where every run keeps its copies, from the environment of this process.
fn root() -> io::Result<PathBuf> {
    if let Some(dir) = std::env::var_os(KEPT_DIR).filter(|v| Path::new(v).is_absolute()) {
        return Ok(PathBuf::from(dir));
    }
    let cache = match std::env::var_os("XDG_CACHE_HOME").filter(|v| Path::new(v).is_absolute()) {
        Some(dir) => PathBuf::from(dir),
        None => match std::env::var_os("HOME").filter(|v| Path::new(v).is_absolute()) {
            Some(home) => PathBuf::from(home).join(".cache"),
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "neither XDG_CACHE_HOME nor HOME names an absolute directory, so no copy of the file can be kept \
                     before it is written; set one",
                ));
            },
        },
    };
    Ok(cache.join("mockspace").join("kept"))
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// This run's directory, once it is made.
static RUN: OnceLock<Result<PathBuf, String>> = OnceLock::new();

/// This run's directory, made once, with the runs older than [`KEPT_FOR`] taken out first.
fn run_dir() -> io::Result<&'static Path> {
    let made = RUN.get_or_init(|| {
        let root = root().map_err(|e| e.to_string())?;
        let now = now();
        prune(&root, now);
        let dir = root.join(format!("{now}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| format!("making {}: {e}", dir.display()))?;
        Ok(dir)
    });
    match made {
        Ok(dir) => Ok(dir.as_path()),
        Err(why) => Err(io::Error::other(why.clone())),
    }
}

/// Makes this run's directory, so a write that follows finds its copy can be made: a caller that
/// must refuse before it writes anything asks this first, and a copy that could not be made then
/// stops nothing half done.
pub fn ready() -> io::Result<()> {
    run_dir().map(|_| ())
}

/// The shell function `ms_keep <path>`, for a program with no engine to run: it copies what stands
/// at `<path>`, an absolute path, into a run directory of its own under the same root and in the
/// same layout as [`keep`] (`<root>/<seconds>-<pid>/1/<absolute path>`), so [`copies_under`] finds
/// it and a later run's [`prune`] takes it out after a day. It says where the copy went on stderr,
/// fails where it could not be made, and never stops the caller, which decides what that means.
pub const SHELL_KEEP: &str = r#"ms_keep() { # <absolute path>
    [ -e "$1" ] || [ -L "$1" ] || return 0
    if [ -n "${MOCKSPACE_KEPT_DIR:-}" ] && [ "${MOCKSPACE_KEPT_DIR#/}" != "$MOCKSPACE_KEPT_DIR" ]; then ms_root=$MOCKSPACE_KEPT_DIR
    elif [ -n "${XDG_CACHE_HOME:-}" ] && [ "${XDG_CACHE_HOME#/}" != "$XDG_CACHE_HOME" ]; then ms_root=$XDG_CACHE_HOME/mockspace/kept
    elif [ -n "${HOME:-}" ] && [ "${HOME#/}" != "$HOME" ]; then ms_root=$HOME/.cache/mockspace/kept
    else echo "mockspace: no absolute XDG_CACHE_HOME or HOME, so $1 is copied nowhere" >&2; return 1
    fi
    ms_to="$ms_root/$(date +%s)-$$/1$1"
    mkdir -p "$(dirname "$ms_to")" && cp -RP "$1" "$ms_to" || { echo "mockspace: $1 could not be copied to $ms_to" >&2; return 1; }
    echo "mockspace: copied $1 to $ms_to" >&2
}"#;

/// Takes out every run directory under `root` whose run started more than [`KEPT_FOR`] seconds
/// before `now`. A name that is not `<seconds>-<pid>` is never touched, and a removal that fails is
/// left for the next run.
pub fn prune(root: &Path, now: u64) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(started) = name.to_str().and_then(|n| n.split_once('-')).and_then(|(s, pid)| {
            pid.parse::<u32>().ok()?;
            s.parse::<u64>().ok()
        }) else {
            continue;
        };
        if now.saturating_sub(started) > KEPT_FOR {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// Copies what stands at `path` (a file, a link, or a directory with all it holds) into this
/// run's directory under its absolute path, before the caller overwrites, changes or removes it.
/// Nothing where nothing stands there. Returns where the copy is.
pub fn keep(path: &Path) -> io::Result<Option<PathBuf>> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        // a path below a file names nothing either, as a removal there finds nothing
        Err(e) if matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::NotADirectory) => return Ok(None),
        Err(e) => return Err(e),
    };
    let abs = std::path::absolute(path)?;
    let rel: PathBuf = abs.components().filter(|c| matches!(c, std::path::Component::Normal(_))).collect();
    // each copy of a run under a number of its own, so a second copy of a path, or of a path below
    // a file copied before, never meets the first
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let to = run_dir()?.join(n.to_string()).join(rel);
    copy(path, &to, &meta)?;
    Ok(Some(to))
}

/// Every copy of `path` kept under `root`, a directory of runs as [`keep`] makes it: oldest run
/// first, and within a run in the order the copies were made. Nothing where `root` is not there.
pub fn copies_under(root: &Path, path: &Path) -> io::Result<Vec<PathBuf>> {
    let runs = match std::fs::read_dir(root) {
        Ok(runs) => runs,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut runs: Vec<(u64, PathBuf)> = runs
        .flatten()
        .filter_map(|r| Some((r.file_name().to_str()?.split_once('-')?.0.parse().ok()?, r.path())))
        .collect();
    runs.sort();
    let mut out = Vec::new();
    for (_, run) in runs {
        out.extend(copies_in_run(&run, path)?);
    }
    Ok(out)
}

/// Every copy of `path` this process kept, in the order it made them.
pub fn copies_of(path: &Path) -> io::Result<Vec<PathBuf>> {
    match RUN.get() {
        Some(Ok(run)) => copies_in_run(run, path),
        _ => Ok(Vec::new()),
    }
}

/// The copies of `path` in the run directory `run`, by the number of each copy.
fn copies_in_run(run: &Path, path: &Path) -> io::Result<Vec<PathBuf>> {
    let rel: PathBuf = std::path::absolute(path)?.components().filter(|c| matches!(c, std::path::Component::Normal(_))).collect();
    let mut found: Vec<(u64, PathBuf)> = match std::fs::read_dir(run) {
        Ok(copies) => copies
            .flatten()
            .filter_map(|n| Some((n.file_name().to_str()?.parse().ok()?, n.path().join(&rel))))
            .filter(|(_, p)| std::fs::symlink_metadata(p).is_ok())
            .collect(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e),
    };
    found.sort();
    Ok(found.into_iter().map(|(_, p)| p).collect())
}

fn copy(from: &Path, to: &Path, meta: &std::fs::Metadata) -> io::Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let kind = meta.file_type();
    if kind.is_symlink() {
        link_like(from, to)
    } else if kind.is_dir() {
        std::fs::create_dir_all(to)?;
        for e in std::fs::read_dir(from)? {
            let e = e?;
            copy(&e.path(), &to.join(e.file_name()), &std::fs::symlink_metadata(e.path())?)?;
        }
        Ok(())
    } else if kind.is_file() {
        std::fs::copy(from, to).map(|_| ())
    } else {
        // a socket or a fifo holds no bytes to keep
        Ok(())
    }
}

/// A link at `to` with the target of the link at `from`, as the target's platform makes one.
#[cfg(unix)]
fn link_like(from: &Path, to: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(std::fs::read_link(from)?, to)
}

/// A link at `to` with the target of the link at `from`: a directory link where `from` points at a
/// directory, else a file link, as Windows tells the two apart.
#[cfg(windows)]
fn link_like(from: &Path, to: &Path) -> io::Result<()> {
    let target = std::fs::read_link(from)?;
    if std::fs::metadata(from).is_ok_and(|m| m.is_dir()) {
        std::os::windows::fs::symlink_dir(target, to)
    } else {
        std::os::windows::fs::symlink_file(target, to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mutation: prune by the modification time of the directory, or take out a name of another
    // shape; a run of yesterday then stays, or a person's own file in the cache goes.
    #[test]
    fn a_run_older_than_a_day_is_taken_out_and_nothing_else() {
        let dir = mockspace_manifest::scratch::tempdir().unwrap();
        let now = 10 * KEPT_FOR;
        for name in [format!("{}-1", now - KEPT_FOR - 1), format!("{}-2", now - KEPT_FOR + 60), "notes".into(), format!("{}-x", now - 2 * KEPT_FOR)] {
            std::fs::create_dir_all(dir.path().join(&name).join("a")).unwrap();
        }
        prune(dir.path(), now);
        let mut left: Vec<String> =
            std::fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        let mut want = vec![format!("{}-2", now - KEPT_FOR + 60), "notes".to_string(), format!("{}-x", now - 2 * KEPT_FOR)];
        want.sort();
        assert_eq!(left, want);
    }

    // Mutation: keep every copy of a run under the path alone; a second copy of a path, or a copy of
    // a path below a file copied before, then meets the first and fails.
    #[test]
    fn a_path_kept_twice_and_one_below_a_file_kept_before_are_each_kept() {
        let dir = mockspace_manifest::scratch::tempdir().unwrap();
        let f = dir.path().join("f");
        std::fs::write(&f, b"one").unwrap();
        let first = keep(&f).unwrap().unwrap();
        std::fs::write(&f, b"two").unwrap();
        let second = keep(&f).unwrap().unwrap();
        std::fs::remove_file(&f).unwrap();
        std::fs::create_dir(&f).unwrap();
        std::fs::write(f.join("under"), b"three").unwrap();
        let third = keep(&f.join("under")).unwrap().unwrap();
        assert_eq!(std::fs::read(first).unwrap(), b"one");
        assert_eq!(std::fs::read(second).unwrap(), b"two");
        assert_eq!(std::fs::read(third).unwrap(), b"three");
    }

    // Mutation: read a path below a file as an error; a removal there, which finds nothing, would
    // then stop on the copy.
    #[test]
    fn nothing_standing_at_a_path_or_below_a_file_is_nothing_to_keep() {
        let dir = mockspace_manifest::scratch::tempdir().unwrap();
        std::fs::write(dir.path().join("file"), b"x").unwrap();
        assert_eq!(keep(&dir.path().join("absent")).unwrap(), None);
        assert_eq!(keep(&dir.path().join("file/under")).unwrap(), None);
    }

    // Mutation: follow a link, or copy a directory's files flat; the copy then holds what the link
    // pointed at, or loses the layout.
    #[cfg(unix)]
    #[test]
    fn a_file_a_link_and_a_directory_are_kept_as_they_stand() {
        let dir = mockspace_manifest::scratch::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(src.join("d/e")).unwrap();
        std::fs::write(src.join("d/e/f"), b"deep").unwrap();
        std::os::unix::fs::symlink("e/f", src.join("d/link")).unwrap();
        let to = dir.path().join("to");
        copy(&src.join("d"), &to, &std::fs::symlink_metadata(src.join("d")).unwrap()).unwrap();
        assert_eq!(std::fs::read(to.join("e/f")).unwrap(), b"deep");
        assert_eq!(std::fs::read_link(to.join("link")).unwrap(), Path::new("e/f"));
    }

    // Mutation: put the shell's copy under another layout, or follow its links; `copies_under`
    // then finds nothing of it, or the copy of the link holds what it pointed at.
    #[cfg(unix)]
    #[test]
    fn the_shell_copy_is_found_where_a_copy_of_the_engine_is_and_keeps_a_link_a_link() {
        let dir = mockspace_manifest::scratch::tempdir().unwrap();
        let root = dir.path().join("kept");
        let src = dir.path().join("inst dir");
        std::fs::create_dir_all(src.join("d")).unwrap();
        std::fs::write(src.join("d/f"), b"the edit").unwrap();
        std::os::unix::fs::symlink("d/f", src.join("link")).unwrap();
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("{SHELL_KEEP}\nms_keep \"$1\""))
            .arg("sh")
            .arg(&src)
            .env(KEPT_DIR, &root)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let copies = copies_under(&root, &src.join("d/f")).unwrap();
        assert_eq!(copies.len(), 1, "{copies:?}");
        assert_eq!(std::fs::read(&copies[0]).unwrap(), b"the edit");
        let link = copies_under(&root, &src.join("link")).unwrap();
        assert_eq!(std::fs::read_link(&link[0]).unwrap(), Path::new("d/f"));
        // a root that cannot be made is said, and the function fails
        std::fs::write(dir.path().join("blocked"), b"a file where the root goes").unwrap();
        let blocked = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("{SHELL_KEEP}\nms_keep \"$1\""))
            .arg("sh")
            .arg(&src)
            .env(KEPT_DIR, dir.path().join("blocked"))
            .output()
            .unwrap();
        assert!(!blocked.status.success() && String::from_utf8_lossy(&blocked.stderr).contains("could not be copied"));
    }
}
