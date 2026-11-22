//! Copies of the files a command is about to overwrite, change or remove, kept in the person's
//! cache for a day.
//!
//! Every write the engine makes over a file it did not make in the same breath goes through
//! [`keep`] first: a file of an instance, the sparse-checkout and exclude files, a hook, the git
//! config, a file a migration rewrites or renames. The copy is a last way back from a case nobody
//! foresaw, not the way the engine keeps an edit (the backup ref of an instance is), so it is plain
//! files under `<cache>/mockspace/kept/<seconds>-<pid>/<n>/<absolute path>`, one directory per run
//! and a number per copy, and a run takes out every directory of a run more than [`KEPT_FOR`]
//! seconds old. `<cache>` is `$XDG_CACHE_HOME`, or `$HOME/.cache`. A copy that cannot be made stops
//! the write: the caller reports it and writes nothing.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// How long a run's copies are kept, in seconds: a day.
pub const KEPT_FOR: u64 = 24 * 60 * 60;

/// The variable that names the directory to keep the copies in instead of the cache; the test
/// binaries set it (`mockspace_manifest::ambient::KEPT_DIR`), so a test never writes the person's cache.
pub const KEPT_DIR: &str = "MOCKSPACE_KEPT_DIR";

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

/// This run's directory, made once, with the runs older than [`KEPT_FOR`] taken out first.
fn run_dir() -> io::Result<&'static Path> {
    static DIR: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    let made = DIR.get_or_init(|| {
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

fn copy(from: &Path, to: &Path, meta: &std::fs::Metadata) -> io::Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let kind = meta.file_type();
    if kind.is_symlink() {
        std::os::unix::fs::symlink(std::fs::read_link(from)?, to)
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

#[cfg(test)]
mod tests {
    use super::*;

    // Mutation: spell the variable otherwise in one of the two crates; the tests then write the
    // person's cache.
    #[test]
    fn the_test_binaries_set_the_variable_this_reads() {
        assert_eq!(KEPT_DIR, mockspace_manifest::ambient::KEPT_DIR);
    }

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
}
