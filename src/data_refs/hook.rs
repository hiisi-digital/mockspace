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
//! holds the edit, so the next engine command says what git took and `restore` brings it back.
//!
//! Neither hook ever stops the person's command: each exits 0 whatever the launcher does, and says a
//! failure on stderr, since a command refused for a copy that could not be made is a stop the person
//! did not ask for, and the next engine command still finds what git wrote over by the bytes the
//! copy kept from before.
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

/// The part of both hooks that runs the copy, never stopping git.
const SNAPSHOT: &str = r#"[ -z "${MOCKSPACE_SNAPSHOT:-}" ] || exit 0
if command -v mock >/dev/null 2>&1; then launcher=mock
elif command -v cargo-mock >/dev/null 2>&1; then launcher=cargo-mock
else
    echo 'mockspace: no launcher on PATH, so the files of the instances are not copied before git writes; the git command goes on' >&2
    exit 0
fi
MOCKSPACE_SNAPSHOT=1 MOCKSPACE_NO_AUTO_ACTIVATE=1 "$launcher" data-refs snapshot </dev/null >&2 ||
    echo 'mockspace: the files of the instances could not be copied before git writes; the git command goes on' >&2
exit 0
"#;

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
    fn text(&self) -> String {
        format!(
            "#!/bin/sh\n{}\n# Never stops the git command it runs in: every way out is exit 0.\n{}{SNAPSHOT}",
            self.mark, self.when
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

/// Writes the hooks into the repository's own hooks directory, and makes sure git reaches them. A
/// hook of the person's own is never written over: every one is named, and nothing is written.
pub(super) fn install(cfg: &Config) -> Result<(), String> {
    let common = common_dir(cfg)?;
    let mut states = Vec::new();
    let mut foreign = Vec::new();
    for m in &MANAGED {
        let (path, state) = m.state(&common)?;
        if matches!(state, Hook::Foreign) {
            foreign.push(format!(
                "{} is a hook of your own, and the copy of the files of an instance before git writes over them needs a \
                 `{}` hook: {}, never stopping git, or move it aside and run `cargo mock data-refs instantiate`",
                path.display(),
                m.event,
                m.wanted
            ));
        }
        states.push((m, path, state));
    }
    if !foreign.is_empty() {
        return Err(foreign.join("\n"));
    }
    for (m, path, state) in states {
        if matches!(&state, Hook::Ours(text) if *text == m.text()) {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        mockspace_store::kept::keep(&path).map_err(|e| format!("keeping a copy of {} before writing over it: {e}", path.display()))?;
        mockspace_manifest::write_executable(&path, m.text()).map_err(|e| format!("writing {}: {e}", path.display()))?;
    }
    // the gate's directory, where `core.hooksPath` is it and a hand-over is missing from it: made
    // here too, so the copy runs from the first command, whatever made the directory before
    if let Some(dir) = hooks_path(cfg)?.filter(|d| mockspace_manifest::gate::is_ours(&d.to_string_lossy())) {
        if MANAGED.iter().any(|m| !dir.join(m.event).is_file()) {
            let _ = mockspace_manifest::gate::install_durable_hooks(&dir, mockspace_manifest::gate::HOOK_VERSION);
        }
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
    for m in &MANAGED {
        let (path, state) = m.state(&common)?;
        match state {
            Hook::Ours(text) if text == m.text() => {},
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
