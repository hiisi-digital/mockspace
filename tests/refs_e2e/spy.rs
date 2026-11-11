//! A `git` in front of the real one on the `PATH` of every command a world runs. It logs each call
//! and fails the calls a test names, so a failure is real and happens at one chosen step, and a
//! cost is counted in calls rather than in time.

use std::path::{Path, PathBuf};

use super::world::World;

pub struct Spy {
    /// One line a call: its subcommand, its arguments and the name of the index file it was given.
    /// A failed call is logged again behind `INJECTED`, so a test can tell the step was reached.
    pub log:  PathBuf,
    /// Lines naming the calls to fail: a subcommand, then optionally a text the logged call must
    /// hold, or with a leading `!` must not.
    pub fail: PathBuf,
    /// The ordinal, counted from the last [`Spy::fail_at`], of the one call to fail.
    at:       PathBuf,
    count:    PathBuf,
}

/// Puts the logging `git` first on the `PATH` of every command `w` runs.
pub fn install(w: &World) -> Spy {
    let base = w.base();
    let spy = Spy {
        log:   base.join("git.log"),
        fail:  base.join("git.fail"),
        at:    base.join("git.fail-at"),
        count: base.join("git.count"),
    };
    let real = w.sh(&base, "sh", &["-c", "command -v git"]).stdout.trim().to_string();
    let body = format!(
        r#"#!/bin/sh
sub=; skip=0
for a in "$@"; do
  if [ $skip = 1 ]; then skip=0; continue; fi
  case "$a" in -C|-c) skip=1;; --*) ;; *) sub=$a; break;; esac
done
line="$sub $* idx=${{GIT_INDEX_FILE##*/}}"
echo "$line" >> '{log}'
if [ -f '{at}' ]; then
  n=$(( $(cat '{count}' 2>/dev/null || echo 0) + 1 ))
  echo $n > '{count}'
  if [ "$(cat '{at}')" = "$n" ]; then
    echo "INJECTED $line" >> '{log}'
    echo "injected failure of git $sub, call $n" >&2
    exit 1
  fi
fi
if [ -f '{fail}' ]; then
  while IFS= read -r want; do
    w=${{want%% *}}; rest=${{want#"$w"}}; rest=${{rest# }}; hit=
    [ "$w" = "$sub" ] || continue
    case "$rest" in
      "") hit=1 ;;
      "!"*) case "$line" in *"${{rest#!}}"*) ;; *) hit=1 ;; esac ;;
      *) case "$line" in *"$rest"*) hit=1 ;; esac ;;
    esac
    if [ -n "$hit" ]; then
      echo "INJECTED $line" >> '{log}'
      echo "injected failure of git $sub" >&2
      exit 1
    fi
  done < '{fail}'
fi
exec '{real}' "$@"
"#,
        log = spy.log.display(),
        fail = spy.fail.display(),
        at = spy.at.display(),
        count = spy.count.display(),
    );
    mockspace_manifest::write_executable(&base.join("shims/git"), body).unwrap();
    spy
}

impl Spy {
    pub fn text(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    /// The calls logged since the last [`Spy::clear`] whose line holds `what`.
    pub fn calls(&self, what: &str) -> usize {
        self.text().lines().filter(|l| l.contains(what)).count()
    }

    /// Every process started as `git` since the last [`Spy::clear`].
    pub fn spawns(&self) -> usize {
        self.text().lines().filter(|l| !l.starts_with("INJECTED ")).count()
    }

    pub fn clear(&self) {
        std::fs::write(&self.log, "").unwrap();
    }

    /// Fails the `n`th call from now, counting from one, and no other.
    pub fn fail_at(&self, n: usize) {
        std::fs::write(&self.count, "0").unwrap();
        std::fs::write(&self.at, n.to_string()).unwrap();
    }

    /// Ends every failure this spy was told to make.
    pub fn stop_failing(&self) {
        let _ = std::fs::remove_file(&self.at);
        let _ = std::fs::remove_file(&self.fail);
    }

    /// Whether a call was failed since the last [`Spy::clear`].
    pub fn injected(&self) -> bool {
        self.calls("INJECTED ") > 0
    }
}

/// The loose objects of the object database of `git_dir`: what was written since the last pack,
/// since a write of an object git already has writes nothing. Counted with `gc.auto` off.
pub fn loose_objects(git_dir: &Path) -> usize {
    let Ok(dirs) = std::fs::read_dir(git_dir.join("objects")) else { return 0 };
    dirs.flatten()
        .filter(|d| d.file_name().len() == 2)
        .map(|d| std::fs::read_dir(d.path()).map(|f| f.count()).unwrap_or(0))
        .sum()
}
