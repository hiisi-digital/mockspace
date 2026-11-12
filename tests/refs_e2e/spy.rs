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
    /// hold, or with a leading `!` must not, or with a trailing `$` must end with.
    pub fail: PathBuf,
    /// The ordinal, counted from the last [`Spy::fail_at`], of the one call to fail.
    at:       PathBuf,
    count:    PathBuf,
    /// There when the call at that ordinal kills the process that made it instead of failing.
    kill:     PathBuf,
    /// There, naming the git directory to watch, when the call at that ordinal is started and the
    /// whole process group is killed while it holds a lock there.
    group:    PathBuf,
    /// There while each call's argument and input bytes are counted into `sizes`.
    measure:  PathBuf,
    /// One line a call while measured: its subcommand, its argument bytes and its input bytes.
    sizes:    PathBuf,
}

/// Puts the logging `git` first on the `PATH` of every command `w` runs.
pub fn install(w: &World) -> Spy {
    let base = w.base();
    let spy = Spy {
        log:   base.join("git.log"),
        fail:  base.join("git.fail"),
        at:    base.join("git.fail-at"),
        count: base.join("git.count"),
        kill:  base.join("git.kill"),
        group: base.join("git.group"),
        measure: base.join("git.measure"),
        sizes: base.join("git.sizes"),
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
  if [ "$(cat '{at}')" = "$n" ] && [ -f '{group}' ]; then
    # Ctrl-C at a terminal reaches every process of the foreground group at once: the caller, and
    # this call while it writes. The call is started, held at half of its input, and the group is
    # killed once git holds a lock of the repository, or when it has not taken one by then.
    echo "GROUP $line" >> '{log}'
    watch=$(cat '{group}')
    pg=$(cut -d' ' -f5 /proc/$$/stat)
    tmp='{log}'.in.$$
    cat > "$tmp"
    fifo='{log}'.fifo.$$
    rm -f "$fifo"; mkfifo "$fifo"
    '{real}' "$@" < "$fifo" &
    g=$!
    exec 4> "$fifo"
    rm -f "$fifo"
    size=$(wc -c < "$tmp")
    head -c $((size / 2)) "$tmp" >&4
    rm -f "$tmp"
    i=0
    while [ $i -lt 200 ] && kill -0 $g 2>/dev/null; do
      if [ -n "$(find "$watch" -path "$watch/objects" -prune -o -name '*.lock' -print 2>/dev/null | head -n 1)" ]; then
        echo "LOCKED $(find "$watch" -path "$watch/objects" -prune -o -name '*.lock' -print 2>/dev/null | head -n 1)" >> '{log}'
        break
      fi
      i=$((i + 1))
    done
    echo "KILLED $line" >> '{log}'
    # dash reads `--` as a process id, so the group goes straight after the signal
    kill -KILL -$pg
    exit 137
  fi
  if [ "$(cat '{at}')" = "$n" ] && [ -f '{kill}' ]; then
    echo "KILLED $line" >> '{log}'
    kill -KILL $PPID
    exit 137
  fi
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
      *'$') case "$line" in *"${{rest%?}}") hit=1 ;; esac ;;
      *) case "$line" in *"$rest"*) hit=1 ;; esac ;;
    esac
    if [ -n "$hit" ]; then
      echo "INJECTED $line" >> '{log}'
      echo "injected failure of git $sub" >&2
      exit 1
    fi
  done < '{fail}'
fi
if [ -f '{measure}' ]; then
  # what the call is handed: the bytes of its arguments and of its input, which grow with the files
  # the caller names to it where the number of calls does not
  tmp='{sizes}'.$$
  cat > "$tmp"
  # the process id of the caller, where a name holds it, counts as one byte, so the count is the
  # same whatever number the system gave the process
  echo "$sub $(printf %s "$*" | sed "s/$PPID/P/g" | wc -c) $(sed "s/$PPID/P/g" "$tmp" | wc -c)" >> '{sizes}'
  exec 3< "$tmp"
  rm -f "$tmp"
  exec '{real}' "$@" <&3 3<&-
fi
exec '{real}' "$@"
"#,
        log = spy.log.display(),
        fail = spy.fail.display(),
        at = spy.at.display(),
        count = spy.count.display(),
        kill = spy.kill.display(),
        group = spy.group.display(),
        measure = spy.measure.display(),
        sizes = spy.sizes.display(),
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
        let marks = ["INJECTED ", "KILLED ", "GROUP ", "LOCKED "];
        self.text().lines().filter(|l| !marks.iter().any(|m| l.starts_with(m))).count()
    }

    pub fn clear(&self) {
        std::fs::write(&self.log, "").unwrap();
        std::fs::write(&self.sizes, "").unwrap();
    }

    /// Counts from now what every call is handed, which [`Spy::handed`] sums.
    pub fn measure_input(&self) {
        std::fs::write(&self.measure, "").unwrap();
    }

    /// The bytes of the arguments and of the input of every call since the last [`Spy::clear`],
    /// while [`Spy::measure_input`] is on.
    pub fn handed(&self) -> usize {
        std::fs::read_to_string(&self.sizes)
            .unwrap_or_default()
            .lines()
            .map(|l| l.split(' ').skip(1).map(|n| n.trim().parse::<usize>().unwrap()).sum::<usize>())
            .sum()
    }

    /// The calls since the last [`Spy::clear`], each as its subcommand and the bytes it was handed,
    /// the largest first, for a failure message.
    pub fn handed_by_call(&self) -> Vec<(String, usize)> {
        let mut out: Vec<(String, usize)> = std::fs::read_to_string(&self.sizes)
            .unwrap_or_default()
            .lines()
            .map(|l| {
                let mut f = l.split(' ');
                let sub = f.next().unwrap_or_default().to_string();
                (sub, f.map(|n| n.trim().parse::<usize>().unwrap()).sum())
            })
            .collect();
        out.sort_by(|a, b| b.1.cmp(&a.1));
        out
    }

    /// Fails the `n`th call from now, counting from one, and no other.
    pub fn fail_at(&self, n: usize) {
        std::fs::write(&self.count, "0").unwrap();
        std::fs::write(&self.at, n.to_string()).unwrap();
    }

    /// Kills the process that makes the `n`th call from now, before the call runs, with a signal no
    /// process handles; the call itself is not made.
    pub fn kill_at(&self, n: usize) {
        std::fs::write(&self.kill, "").unwrap();
        self.fail_at(n);
    }

    /// Starts the `n`th call from now, holds it at half of its input, and once it holds a lock under
    /// `git_dir` kills every process of the group the command runs in, as Ctrl-C at a terminal reaches
    /// them all, with a signal none of them handles: the lock or half-written file git was writing is
    /// left where it is. A call that takes no lock is killed once it ends or after a while.
    pub fn group_kill_at(&self, n: usize, git_dir: &Path) {
        std::fs::write(&self.group, git_dir.as_os_str().as_encoded_bytes()).unwrap();
        self.fail_at(n);
    }

    /// Ends every failure and kill this spy was told to make.
    pub fn stop_failing(&self) {
        let _ = std::fs::remove_file(&self.at);
        let _ = std::fs::remove_file(&self.kill);
        let _ = std::fs::remove_file(&self.group);
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
