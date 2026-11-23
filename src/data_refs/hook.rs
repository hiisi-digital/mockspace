//! The hooks that copy the files of every instance before git writes over them.
//!
//! git moves `ORIG_HEAD` in a merge, a rebase and a reset, and the `reference-transaction` hook
//! runs `data-refs snapshot` when it sees that move, which keeps every file of each instance in its
//! backup ref (see `mockspace_store::Instance::guard`). A merge moves it before it writes the work
//! tree, so that copy holds what the merge writes over; a reset and a rebase move it after they
//! write, so for those the copy that counts is the one taken on the move before them.
//!
//! The `post-checkout` hook runs the same copy after a checkout or a switch moved `HEAD`. A checkout
//! of a commit from before the migration leaves an edited instance file on disk, skipped, where the
//! commit tracks it (`sparse.expectFilesOutsideOfPatterns`); the plain way back to a branch that does
//! not track it then deletes it, and no reference moves before that. The copy taken on the way there
//! holds the edit, so the next engine command says what git took and `restore` brings it back. An
//! edit made there after it is copied by the monitor of `monitor`, before git reads the work tree.
//!
//! Neither hook ever stops the person's command: each exits 0 whatever the engine does, and says a
//! failure on stderr, since a command refused for a copy that could not be made is a stop the person
//! did not ask for, and the next engine command still finds what git wrote over by the bytes the
//! copy kept from before.
//!
//! Each runs the engine that installed it, by its absolute path, and never the launcher on the
//! `PATH`: at a commit from before the migration the launcher reads that commit's pin, which can
//! name an engine with no `snapshot`, and builds it in the person's checkout where the cache does not
//! hold it. Where that engine is gone, as after a clean of the cache, or fails, the hook copies the
//! directories of the instances as they stand into the person's cache, which needs nothing but a
//! shell, and says so; `verify` names a hook whose engine is gone, and `instantiate` writes it again
//! for the engine there is now.
//!
//! The `reference-transaction` hook runs while the transaction that moves `ORIG_HEAD` holds its
//! locks. Under the reftable backend that is the lock of the whole stack, the backup ref's included,
//! so `snapshot` makes the copy as objects and names it beside the instance's record, and the next
//! engine command folds it into the ref (see `mockspace_store`'s `instance/pending.rs`).
//!
//! The hooks are the repository's own, in `<git-common-dir>/hooks`, which no other repository reads.
//! Where `core.hooksPath` points at the gate's durable directory, the gate's hook of the same name
//! there hands over to it (`mockspace_manifest::gate::CHAIN_HOOKS`), and the same where git-shook
//! dispatches and the gate is one of its entries. The gate's directory is made again by every engine
//! run, so a version bump or a clean of the build leaves the hand-over in place by the next command,
//! and `verify` says where git would not reach a hook.

use std::path::PathBuf;

use mockspace_store::Git;

use crate::config::Config;

/// The file in a worktree's git directory naming, one absolute path to a line, the directories of
/// the instances it last installed, which the hooks and the monitor copy as they stand where the
/// engine that installed them is gone.
pub const DIRS: &str = "mock-data-refs.dirs";

/// The engine the hooks and the monitor run, by the absolute path of the engine that installs them,
/// and the mock directory from the top of a worktree. Never the launcher on the `PATH`: at a commit
/// from before the migration the launcher reads that commit's pin, which can name an engine with no
/// `snapshot`, and builds it in the person's checkout where the cache does not hold it.
pub(super) struct Engine {
    exe:      PathBuf,
    mock_rel: String,
}

impl Engine {
    /// The engine of this run, and the mock directory of `cfg`.
    pub(super) fn of(cfg: &Config) -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|e| format!("the path of the engine that is running is not known: {e}"))?;
        let exe = exe.canonicalize().unwrap_or(exe);
        let mock_rel = match cfg.mock_dir.strip_prefix(&cfg.repo_root) {
            Ok(rel) => rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"),
            Err(_) => cfg.mock_dir.to_string_lossy().into_owned(),
        };
        Ok(Engine { exe, mock_rel })
    }

    /// The engine's path, as text.
    pub(super) fn exe_text(&self) -> String {
        self.exe.to_string_lossy().into_owned()
    }

    /// The same mock directory, run by the engine at `exe`.
    fn at(&self, exe: PathBuf) -> Self {
        Engine { exe, mock_rel: self.mock_rel.clone() }
    }

    /// The lines of shell that run the copy and never stop git: the engine by its path, and where
    /// it is gone or fails, every directory the worktree's [`DIRS`] names, copied as it stands into
    /// the person's cache (`mockspace_store::kept::SHELL_KEEP`). Every way out is `exit 0`. Where
    /// `marks`, the fallback also says for the monitor, as `snapshot` would, that the index tracks a
    /// file of an instance, so the monitor goes on copying at a commit from before the migration
    /// with no engine to say it; the monitor itself never asks git that inside git's own read.
    pub(super) fn snapshot(&self, marks: bool) -> String {
        let marks = if marks {
            format!(
                "tracked=$(git rev-parse --path-format=absolute --git-path {} 2>/dev/null) && [ ! -e \"$tracked\" ] &&\n    \
                 while IFS= read -r d; do [ -n \"$d\" ] && [ -n \"$(git -C \"$top\" ls-files -- \"$d\" 2>/dev/null | head -n 1)\" ] && \
                 {{ : >\"$tracked\"; break; }}; done <\"$list\"\n",
                super::monitor::TRACKED
            )
        } else {
            String::new()
        };
        format!(
            r#"[ -z "${{MOCKSPACE_SNAPSHOT:-}}" ] || exit 0
engine={engine}
mock={mock}
top=$(git rev-parse --show-toplevel 2>/dev/null) || exit 0
case $mock in "") dir=$top ;; /*) dir=$mock ;; *) dir=$top/$mock ;; esac
if [ -x "$engine" ]; then
    MOCKSPACE_SNAPSHOT=1 MOCKSPACE_NO_AUTO_ACTIVATE=1 "$engine" --dir "$dir" data-refs snapshot </dev/null >&2 && exit 0
    echo "mockspace: $engine could not copy the files of the instances before git writes; the directories of the instances are copied as they stand, and the git command goes on" >&2
else
    echo "mockspace: the engine that installed this, $engine, is gone, so the directories of the instances are copied as they stand and the git command goes on; \`cargo mock data-refs instantiate\` installs it for the engine there is now" >&2
fi
{keep}
list=$(git rev-parse --path-format=absolute --git-path {DIRS} 2>/dev/null) && [ -f "$list" ] || exit 0
{marks}while IFS= read -r d; do [ -z "$d" ] || ms_keep "$d"; done <"$list"
exit 0
"#,
            engine = sh_quote(&self.exe.to_string_lossy()),
            mock = sh_quote(&self.mock_rel),
            keep = mockspace_store::kept::SHELL_KEEP,
        )
    }
}

/// `s` as one word of shell: in single quotes, each single quote spelled `'\''`.
pub(super) fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The word [`sh_quote`] wrote, `None` where `s` is not one.
pub(super) fn sh_unquote(s: &str) -> Option<String> {
    let inner = s.strip_prefix('\'')?.strip_suffix('\'')?;
    let text = inner.replace(r"'\''", "\u{0}");
    (!text.contains('\'')).then(|| text.replace('\u{0}', "'"))
}

/// Whether `text`, a program carrying `mark`, is one this writes, current or live: what `write`
/// gives for this run's engine, or for the engine it names on its `engine=` line where that engine
/// is still there. Two engines installing in turn, as two worktrees on two pins do, each leave a
/// program that works.
pub(super) fn current_or_live(text: &str, mark: &str, now: &Engine, write: impl Fn(&Engine) -> String) -> bool {
    if !text.contains(mark) {
        return false;
    }
    if text == write(now) {
        return true;
    }
    let named = text.lines().find_map(|l| l.strip_prefix("engine=")).and_then(sh_unquote).map(PathBuf::from);
    named.is_some_and(|exe| is_executable(&exe) && text == write(&now.at(exe)))
}

fn is_executable(p: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        std::fs::metadata(p).is_ok_and(|m| m.is_file())
    }
}

/// One hook the engine keeps in the repository's own hooks directory.
struct Managed {
    /// The git event, and the file name.
    event:     &'static str,
    /// The line that marks the hook as the one this writes.
    mark:      &'static str,
    /// Earlier marks of the same hook, which `install` replaces as its own.
    old_marks: &'static [&'static str],
    /// When it runs the copy, as lines of shell that `exit 0` where it does not.
    when:      &'static str,
    /// What a hook of the person's own has to do instead, for the refusal that names it.
    wanted:    &'static str,
}

const MANAGED: [Managed; 2] = [
    Managed {
        event:     "reference-transaction",
        mark:      "# mockspace data refs: copies the files of each instance before git writes over them (managed, do not edit)",
        old_marks: &["# mockspace data refs: copies an edited instance file before git writes over it (managed, do not edit)"],
        when:      "[ \"$1\" = prepared ] || exit 0\nmoved=\nwhile read -r _ _ ref; do\n    [ \"$ref\" = ORIG_HEAD ] && moved=1\ndone\n[ -n \"$moved\" ] || exit 0\n",
        wanted:    "have its `prepared` case run `mock data-refs snapshot` when a line of its input ends in ` ORIG_HEAD`",
    },
    Managed {
        event:     "post-checkout",
        mark:      "# mockspace data refs: copies the files of each instance after a checkout moved HEAD (managed, do not edit)",
        old_marks: &[],
        // a branch checkout that moved HEAD from a commit; a new worktree or a clone has nothing to keep
        when:      "[ \"$3\" = 1 ] || exit 0\n[ \"$1\" != \"$2\" ] || exit 0\ncase \"$1\" in *[!0]*) ;; *) exit 0 ;; esac\n",
        wanted:    "have it run `mock data-refs snapshot` when its third argument is 1 and its first two differ",
    },
];

impl Managed {
    fn text(&self, engine: &Engine) -> String {
        format!(
            "#!/bin/sh\n{}\n# Never stops the git command it runs in: every way out is exit 0.\n{}{}",
            self.mark,
            self.when,
            engine.snapshot(true)
        )
    }

    fn state(&self, common: &std::path::Path) -> Result<(PathBuf, Hook), String> {
        let path = common.join("hooks").join(self.event);
        let state = match std::fs::read_to_string(&path) {
            Ok(text) if text.contains(self.mark) || self.old_marks.iter().any(|m| text.contains(m)) => Hook::Ours(text),
            Ok(_) => Hook::Foreign,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Hook::Absent,
            Err(e) => return Err(format!("reading {}: {e}", path.display())),
        };
        Ok((path, state))
    }
}

fn git(cfg: &Config, args: &[&str]) -> Result<mockspace_store::Out, String> {
    Git::new(cfg.repo_root.clone()).run(args, &[], None).map_err(|e| e.to_string())
}

fn common_dir(cfg: &Config) -> Result<PathBuf, String> {
    let out = git(cfg, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    if out.ok() { Ok(PathBuf::from(out.text())) } else { Err(format!("git rev-parse --git-common-dir: {}", out.stderr)) }
}

/// What stands where a hook belongs.
enum Hook {
    Absent,
    Ours(String),
    Foreign,
}

/// The hooks [`write`] writes, decided with nothing written.
pub(super) struct Planned {
    engine: Engine,
    write:  Vec<(&'static Managed, PathBuf)>,
    gate:   Option<PathBuf>,
}

/// What [`write`] does, or the refusal that stops it: a hook of the person's own is never written
/// over, and every one is named.
pub(super) fn plan(cfg: &Config) -> Result<Planned, String> {
    let engine = Engine::of(cfg)?;
    let common = common_dir(cfg)?;
    let mut write = Vec::new();
    let mut foreign = Vec::new();
    for m in &MANAGED {
        let (path, state) = m.state(&common)?;
        match state {
            Hook::Foreign => foreign.push(format!(
                "{} is a hook of your own, and the copy of the files of an instance before git writes over them needs a \
                 `{}` hook: {}, never stopping git, or move it aside and run `cargo mock data-refs instantiate`",
                path.display(),
                m.event,
                m.wanted
            )),
            Hook::Ours(text) if text == m.text(&engine) => {},
            _ => write.push((m, path)),
        }
    }
    if !foreign.is_empty() {
        return Err(foreign.join("\n"));
    }
    // the gate's directory, where `core.hooksPath` is it and a hand-over is missing from it: made
    // here too, so the copy runs from the first command, whatever made the directory before
    let gate = hooks_path(cfg)?
        .filter(|d| mockspace_manifest::gate::is_ours(&d.to_string_lossy()))
        .filter(|d| MANAGED.iter().any(|m| !d.join(m.event).is_file()));
    Ok(Planned { engine, write, gate })
}

/// Writes the hooks [`plan`] decided into the repository's own hooks directory, each kept in the
/// cache first, and makes sure git reaches them.
pub(super) fn write(planned: Planned) -> Result<(), String> {
    for (m, path) in planned.write {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        mockspace_store::kept::keep(&path).map_err(|e| format!("keeping a copy of {} before writing over it: {e}", path.display()))?;
        mockspace_manifest::write_executable(&path, m.text(&planned.engine)).map_err(|e| format!("writing {}: {e}", path.display()))?;
    }
    if let Some(dir) = planned.gate {
        let _ = mockspace_manifest::gate::install_durable_hooks(&dir, mockspace_manifest::gate::HOOK_VERSION);
    }
    Ok(())
}

/// `core.hooksPath`, absolute, where it is set.
fn hooks_path(cfg: &Config) -> Result<Option<PathBuf>, String> {
    // a failed read is an error, never read as `core.hooksPath` being unset: the hand-over would
    // be neither made nor checked
    let set = Git::new(cfg.repo_root.clone()).config_get(&[], "core.hooksPath").map_err(|e| e.to_string())?;
    if set.is_none_or(|s| s.is_empty()) {
        return Ok(None);
    }
    let out = git(cfg, &["rev-parse", "--path-format=absolute", "--git-path", "hooks"])?;
    if out.ok() { Ok(Some(PathBuf::from(out.text()))) } else { Err(format!("git rev-parse --git-path hooks: {}", out.stderr)) }
}

/// Whether every hook is there and git runs it: where `core.hooksPath` is set, the directory it
/// names has a hook that hands over to it (the gate's, or git-shook's with the gate registered).
pub(super) fn check(cfg: &Config) -> Result<(), String> {
    let common = common_dir(cfg)?;
    let dir = hooks_path(cfg)?;
    let engine = Engine::of(cfg)?;
    for m in &MANAGED {
        let (path, state) = m.state(&common)?;
        match state {
            Hook::Ours(text) if current_or_live(&text, m.mark, &engine, |e| m.text(e)) => {},
            Hook::Foreign => {
                return Err(format!(
                    "{} is a hook of your own that does not copy the files of an instance before git writes over them; \
                     see `cargo mock data-refs instantiate`",
                    path.display()
                ));
            },
            _ => {
                return Err(format!(
                    "this worktree has no `{}` hook that copies the files of an instance before git writes over them \
                     ({}); `cargo mock data-refs instantiate` installs it",
                    m.event,
                    path.display()
                ));
            },
        }
        let Some(dir) = &dir else { continue };
        let shook_entry = common.join("shook").join("entries").join(m.event).join("00-mockspace");
        let at = dir.join(m.event);
        let hands_over = std::fs::read_to_string(&at).is_ok_and(|t| t.contains("exec \"$own\" \"$@\"") || t.contains(m.mark));
        let shook = dir.canonicalize().ok() == common.join("shook").join("hooks").canonicalize().ok();
        if !(hands_over || (shook && shook_entry.is_file())) {
            return Err(format!(
                "core.hooksPath points at {}, which has no `{}` hook that runs {}, so git never copies the files of an \
                 instance before it writes over them; a `cargo mock` run puts the gate's there, and a directory of your \
                 own needs one that runs it",
                dir.display(),
                m.event,
                path.display()
            ));
        }
    }
    Ok(())
}
