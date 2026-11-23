//! The monitor git asks before it reads the work tree, which copies the files of each instance while
//! the index of this worktree tracks any of them.
//!
//! At a commit from before the migration the index tracks the files of an instance, skipped by the
//! sparse checkout, and the plain way back to a branch that does not track them deletes them. No
//! reference moves before that delete, and the `post-checkout` hook runs after it, so an edit made
//! there after the copy the hook took on the way in would be in no copy. git asks the program
//! `core.fsmonitor` names which files changed before it reads the index into a checkout, a switch, a
//! reset or any other command, and so before it touches the work tree; that program is this one.
//!
//! It runs `data-refs snapshot` only where the last snapshot of this worktree found its index
//! tracking a file of an instance, which it says by a file in the worktree's git directory, so every
//! other git command costs one `git rev-parse`. It runs the engine that installed it by its absolute
//! path, as the hooks do (`hook::Engine`). A git the engine itself runs copies nothing, since the
//! engine copies the instance itself and a snapshot inside it would race it. Like the hooks, it never
//! stops the git command.
//!
//! git runs one program at `core.fsmonitor`, so a monitor of the person's own and this one meet
//! there. git-shook runs both (op, 12 November: "Using git-shook should remove the clash"): its
//! dispatcher is `core.fsmonitor`, this monitor is an entry of it that does not answer, and the
//! person's own is the entry that answers git. So:
//!
//! - with `core.fsmonitor` unset, or this monitor's, and git-shook not holding the hooks, the
//!   monitor is `core.fsmonitor` and answers git itself: that any file may have changed (`/`), by
//!   the protocol version git asks for, which is what git reads with no monitor at all;
//! - where git-shook holds the hooks, or `core.fsmonitor` names its dispatcher, the monitor is
//!   registered with it through a manifest of this clone's own, and the dispatcher is the setting;
//! - where `core.fsmonitor` names a monitor of the person's own, a watchman hook or any command,
//!   that command becomes the entry that answers, in a second manifest, and the dispatcher the
//!   setting; git-shook is installed for the clone where it does not hold the hooks yet, and where
//!   the gate held them, the gate is registered with it at once;
//! - git's own daemon (`core.fsmonitor=true`) has no program a dispatcher could run, so where
//!   git-shook holds the hooks its dispatcher answers that any file may have changed, and where it
//!   does not the monitor takes the setting; both say so, and what was set is in the copy of the
//!   config kept in the cache.
//!
//! Every refusal is made by [`plan`] before anything is written: a monitor of the person's own with no
//! `git shook` to run, a command git-shook could not hold, and an answering entry git-shook already
//! has beside a command of the person's.

use std::path::{Path, PathBuf};

use mockspace_store::Git;

use super::hook::{Engine, current_or_live, sh_quote, sh_unquote};
use crate::config::Config;

/// The variable the engine sets for itself, and so for everything it runs, so the monitor of a git
/// it runs copies nothing.
pub const IN_ENGINE: &str = "MOCKSPACE_IN_ENGINE";

/// The file in this worktree's git directory that says its index tracks a file of an instance.
pub const TRACKED: &str = "mock-data-refs.tracked";

/// The monitor's file name in the repository's own hooks directory.
const NAME: &str = "mockspace-fsmonitor";

const MARK: &str = "# mockspace data refs: copies the files of each instance before git reads the work tree (managed, do not edit)";

/// The git event git-shook dispatches `core.fsmonitor` as.
const EVENT: &str = "fsmonitor-watchman";

/// The name the monitor registers under with git-shook. git-shook runs an event's entries sorted by
/// name and stops at the first that fails, so the name sorts before every other, `00-existing`
/// included, and the copy runs whatever the answering entry does.
const ENTRY: &str = "0-mockspace-data-refs";

/// The name the person's own monitor is registered under, as the entry that answers git.
const THEIRS: &str = "0-mockspace-kept-fsmonitor";

/// Says that this process is the engine. Every process it starts inherits the variable, so a git
/// it runs, and the monitor that git asks, copy nothing.
///
/// # Safety
///
/// The environment is process-global; call it before any other thread exists.
pub unsafe fn mark_engine_process() {
    // SAFETY: the caller guarantees no other thread exists yet.
    unsafe { std::env::set_var(IN_ENGINE, "1") };
}

/// The monitor's text: the copy where this worktree's index tracks a file of an instance, then the
/// answer that any file may have changed, by the protocol version git asks for (`$1`): version 1
/// wants the paths alone, version 2 a token first, and `/` is every path. Run as an entry of
/// git-shook, its answer is thrown away.
fn text(engine: &Engine) -> String {
    format!(
        "#!/bin/sh\n{MARK}\nengine={}\n# Never stops the git command it runs in, and answers that any file may have changed, as git \
         reads the work tree with no monitor.\nif [ -z \"${{{IN_ENGINE}:-}}\" ] && tracked=$(git rev-parse --git-path \
         {TRACKED} 2>/dev/null) && [ -e \"$tracked\" ]; then\n    ( {} ) </dev/null >&2\nfi\nif [ \"${{1:-}}\" = 1 ]; then \
         printf '/\\0'; else printf '%s\\0/\\0' \"${{2:-mockspace}}\"; fi\n",
        sh_quote(&engine.exe_text()),
        engine.snapshot(false).trim_end().replace('\n', "\n    ")
    )
}

fn git(cfg: &Config, args: &[&str]) -> Result<String, String> {
    Git::new(cfg.repo_root.clone()).must(args, &[], None).map(|o| o.text()).map_err(|e| e.to_string())
}

fn common_dir(cfg: &Config) -> Result<PathBuf, String> {
    git(cfg, &["rev-parse", "--path-format=absolute", "--git-common-dir"]).map(PathBuf::from)
}

/// A config value, `None` where it is not set; a failed read is an error.
fn config(cfg: &Config, key: &str) -> Result<Option<String>, String> {
    Git::new(cfg.repo_root.clone()).config_get(&[], key).map_err(|e| e.to_string())
}

fn canon(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// The path a `core.fsmonitor` value names, where it is one path, quoted or not.
fn named_path(value: &str, repo_root: &Path) -> Option<PathBuf> {
    let v = value.trim();
    let bare = sh_unquote(v).or_else(|| (!v.contains(char::is_whitespace)).then(|| v.to_string()))?;
    let p = PathBuf::from(bare);
    Some(canon(&if p.is_absolute() { p } else { repo_root.join(p) }))
}

/// What `core.fsmonitor` holds.
#[derive(Debug, PartialEq)]
enum Setting {
    /// Unset, empty or off: git asks no monitor.
    Off,
    /// This monitor.
    Ours,
    /// git-shook's dispatcher.
    Dispatcher,
    /// git's own daemon, which no hook can ask.
    Daemon,
    /// A command of the person's own.
    Theirs(String),
}

fn setting_of(value: Option<&str>, cfg: &Config, ours: &Path, dispatcher: &Path) -> Setting {
    let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) else { return Setting::Off };
    match v.to_ascii_lowercase().as_str() {
        "false" | "no" | "off" | "0" => return Setting::Off,
        "true" | "yes" | "on" | "1" => return Setting::Daemon,
        _ => {},
    }
    match named_path(v, &cfg.repo_root) {
        Some(p) if p == canon(ours) => Setting::Ours,
        Some(p) if p == canon(dispatcher) => Setting::Dispatcher,
        _ => Setting::Theirs(v.to_string()),
    }
}

/// How git reaches the monitor.
#[derive(Debug)]
enum Route {
    /// `core.fsmonitor` names the monitor, which answers git.
    Direct,
    /// git-shook's dispatcher is `core.fsmonitor`, and the monitor is an entry of it that does not
    /// answer; `theirs` is the person's command, registered as the entry that answers.
    Shook {
        theirs: Option<String>,
        /// The gate's directory, where git-shook takes the hooks from it and the gate is registered
        /// with it at once.
        gate:   Option<PathBuf>,
    },
}

/// What [`write`] does, decided with nothing written.
pub(super) struct Planned {
    engine:     Engine,
    at:         PathBuf,
    shook:      PathBuf,
    dispatcher: PathBuf,
    current:    Setting,
    was:        Option<String>,
    route:      Route,
}

/// Whether `git shook` runs here.
fn shook_runs(cfg: &Config) -> bool {
    std::process::Command::new("git")
        .args(["shook", "version"])
        .current_dir(&cfg.repo_root)
        .stdin(std::process::Stdio::null())
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Where the monitor goes and how git reaches it, or the refusal that stops it.
pub(super) fn plan(cfg: &Config) -> Result<Planned, String> {
    let engine = Engine::of(cfg)?;
    let common = common_dir(cfg)?;
    let at = common.join("hooks").join(NAME);
    let shook = common.join("shook");
    let dispatcher = shook.join("hooks").join(EVENT);
    let was = config(cfg, "core.fsmonitor")?;
    let current = setting_of(was.as_deref(), cfg, &at, &dispatcher);
    let hooks_path = config(cfg, "core.hooksPath")?.filter(|p| !p.is_empty());
    let shook_holds = hooks_path.as_deref().is_some_and(|p| canon(Path::new(p)) == canon(&shook.join("hooks")));
    let no_clash = matches!(current, Setting::Off | Setting::Ours | Setting::Daemon);
    // asked only where git-shook could be used, since it starts a program
    let runs = (shook_holds || !no_clash) && shook_runs(cfg);
    let route = match &current {
        _ if no_clash && !runs => Route::Direct,
        Setting::Off | Setting::Ours | Setting::Daemon => Route::Shook { theirs: None, gate: None },
        Setting::Dispatcher if runs => Route::Shook { theirs: None, gate: None },
        Setting::Dispatcher => {
            return Err(format!(
                "core.fsmonitor names git-shook's dispatcher ({}), and `git shook` does not run here, so the monitor that \
                 copies the files of an instance before git reads the work tree cannot be registered with it; put \
                 git-shook on the PATH and run `cargo mock data-refs instantiate`",
                dispatcher.display()
            ));
        },
        Setting::Theirs(cmd) => {
            if !runs {
                return Err(format!(
                    "core.fsmonitor is set to {cmd}, a monitor of your own, and git runs one program there; git-shook runs \
                     it beside the monitor that copies the files of an instance before git reads the work tree, so put \
                     git-shook on the PATH and run `cargo mock data-refs instantiate`; nothing was changed"
                ));
            }
            if cmd.contains(['\n', '\r']) {
                return Err(format!(
                    "core.fsmonitor holds a line break ({cmd:?}), which git-shook cannot hold as an argument, so it cannot \
                     run beside the monitor that copies the files of an instance; nothing was changed"
                ));
            }
            theirs_route(cfg, &shook, cmd, shook_holds, hooks_path.as_deref())?
        },
    };
    Ok(Planned { engine, at, shook, dispatcher, current, was, route })
}

/// The route of a command of the person's own: registered as the entry that answers, unless the
/// hook git-shook adopts as it takes the hooks is that command already, and refused where git-shook
/// has an entry that answers other than it.
fn theirs_route(cfg: &Config, shook: &Path, cmd: &str, shook_holds: bool, hooks_path: Option<&str>) -> Result<Route, String> {
    let entries = shook.join("entries").join(EVENT);
    let answers = std::fs::read_to_string(shook.join("answers").join(EVENT)).ok();
    let answering = answers.as_deref().map(str::trim).filter(|a| !a.is_empty() && *a != THEIRS && entries.join(a).is_file());
    let existing = entries.join("00-existing");
    let named = named_path(cmd, &cfg.repo_root);
    // the hook git-shook keeps from the directory it takes the hooks from
    let adopted = if shook_holds {
        None
    } else {
        let old = match hooks_path {
            Some(p) if Path::new(p).is_absolute() => PathBuf::from(p),
            Some(p) => cfg.repo_root.join(p),
            None => common_dir(cfg)?.join("hooks"),
        };
        Some(old.join(EVENT)).filter(|h| h.is_file())
    };
    if let Some(h) = &adopted {
        if named.as_ref() == Some(&canon(h)) {
            // git-shook keeps that hook as `00-existing`, the entry that answers
            return Ok(Route::Shook { theirs: None, gate: gate_of(cfg, hooks_path) });
        }
    }
    let other = answering.map(str::to_string).or_else(|| existing.is_file().then(|| "00-existing".to_string())).or_else(|| {
        adopted.as_ref().map(|h| h.display().to_string())
    });
    if let Some(other) = other {
        return Err(format!(
            "core.fsmonitor is set to {cmd}, a monitor of your own, and git-shook already has `{other}` answering git at \
             `{EVENT}`; git takes one answer, so point core.fsmonitor at git-shook's dispatcher or take that entry out, then \
             run `cargo mock data-refs instantiate`; nothing was changed"
        ));
    }
    Ok(Route::Shook { theirs: Some(cmd.to_string()), gate: if shook_holds { None } else { gate_of(cfg, hooks_path) } })
}

/// The gate's directory, where `core.hooksPath` is it.
fn gate_of(cfg: &Config, hooks_path: Option<&str>) -> Option<PathBuf> {
    hooks_path.filter(|p| mockspace_manifest::gate::is_ours(p)).map(|p| cfg.repo_root.join(p))
}

/// A string as git-shook's manifest reader takes one: quoted, a backslash and a quote escaped.
fn toml_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The manifest that registers the monitor with git-shook, as an entry that does not answer.
fn manifest(at: &Path) -> String {
    format!(
        "# mockspace data refs: the monitor that copies the files of each instance before git reads the work tree (managed)\n\
         name = {}\n\n[hooks.{EVENT}]\nrun = [{}]\n",
        toml_quote(ENTRY),
        toml_quote(&at.to_string_lossy())
    )
}

/// The manifest that registers the person's own monitor with git-shook as the entry that answers,
/// run as git runs `core.fsmonitor`: through a shell, with git's arguments after it.
fn theirs_manifest(cmd: &str) -> String {
    format!(
        "# mockspace data refs: core.fsmonitor as it was set, run beside the monitor and answering git (managed)\n\
         name = {}\n\n[hooks.{EVENT}]\nanswers = true\nrun = [\"sh\", \"-c\", {}, {}]\n",
        toml_quote(THEIRS),
        toml_quote(&format!("{cmd} \"$@\"")),
        toml_quote(cmd)
    )
}

/// Writes the monitor and the setting [`plan`] decided, in the config `scope` names (`--worktree`
/// where per-worktree config is on). `keep_config` keeps the config files before a setting changes.
pub(super) fn write(cfg: &Config, planned: Planned, scope: &[&str], keep_config: impl Fn() -> Result<(), String>) -> Result<(), String> {
    let Planned { engine, at, shook, dispatcher, current, was, route } = planned;
    let want = text(&engine);
    if std::fs::read_to_string(&at).ok().as_deref() != Some(want.as_str()) {
        if let Some(parent) = at.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        mockspace_store::kept::keep(&at).map_err(|e| format!("keeping a copy of {} before writing over it: {e}", at.display()))?;
        mockspace_manifest::write_executable(&at, want).map_err(|e| format!("writing {}: {e}", at.display()))?;
    }
    let set_to = |value: &Path| -> Result<(), String> {
        keep_config()?;
        let value = sh_quote(&value.to_string_lossy());
        let mut args = vec!["config"];
        args.extend_from_slice(scope);
        args.extend(["core.fsmonitor", value.as_str()]);
        git(cfg, &args).map(|_| ())
    };
    match route {
        Route::Direct => {
            if current != Setting::Ours {
                if current == Setting::Daemon {
                    eprintln!(
                        "data-refs: core.fsmonitor was {}, git's own daemon, which answers no program git-shook could run \
                         beside the monitor; the monitor answers now, that any file may have changed",
                        was.as_deref().unwrap_or_default()
                    );
                }
                set_to(&at)?;
            }
        },
        Route::Shook { theirs, gate } => {
            register(cfg, &shook, &at, theirs.as_deref(), &keep_config)?;
            if let Some(dir) = gate {
                crate::bootstrap::register_gate_with_shook(&cfg.repo_root, &dir)?;
            }
            if current != Setting::Dispatcher {
                match &current {
                    Setting::Daemon => eprintln!(
                        "data-refs: core.fsmonitor was {}, git's own daemon, which answers no program git-shook could run; \
                         git-shook's dispatcher answers now, that any file may have changed",
                        was.as_deref().unwrap_or_default()
                    ),
                    Setting::Theirs(cmd) => eprintln!(
                        "data-refs: core.fsmonitor was {cmd}; git-shook runs it now as the entry that answers git, beside the \
                         monitor that copies the files of the instances"
                    ),
                    _ => {},
                }
                set_to(&dispatcher)?;
            }
        },
    }
    Ok(())
}

/// Writes the manifests of this clone's own and has git-shook register them. A registration that
/// fails puts every manifest back as it was, so git-shook's refusal leaves nothing behind.
fn register(cfg: &Config, shook: &Path, at: &Path, theirs: Option<&str>, keep_config: &impl Fn() -> Result<(), String>) -> Result<(), String> {
    let dir = shook.join("manifests");
    let mut files = vec![(dir.join(format!("{ENTRY}.toml")), manifest(at), ENTRY)];
    if let Some(cmd) = theirs {
        files.push((dir.join(format!("{THEIRS}.toml")), theirs_manifest(cmd), THEIRS));
    }
    let entry_runs_it = std::fs::read_to_string(shook.join("entries").join(EVENT).join(ENTRY)).is_ok_and(|t| t.lines().nth(1) == Some(&*at.to_string_lossy()));
    let stale: Vec<_> = files.iter().filter(|(p, text, _)| std::fs::read_to_string(p).ok().as_deref() != Some(text.as_str())).collect();
    if stale.is_empty() && entry_runs_it {
        return Ok(());
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let mut before = Vec::new();
    for (p, text, _) in &files {
        mockspace_store::kept::keep(p).map_err(|e| format!("keeping a copy of {} before writing over it: {e}", p.display()))?;
        before.push((p.clone(), std::fs::read(p).ok()));
        std::fs::write(p, text).map_err(|e| format!("writing {}: {e}", p.display()))?;
    }
    // git-shook takes `core.hooksPath` where it does not hold it yet
    keep_config()?;
    let mut args = vec!["shook", "install", "--yes"];
    for (_, _, name) in &files {
        args.extend(["--only", name]);
    }
    let out = std::process::Command::new("git").args(&args).current_dir(&cfg.repo_root).stdin(std::process::Stdio::null()).output();
    let failed = match out {
        Ok(o) if o.status.success() => None,
        Ok(o) => Some(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => Some(e.to_string()),
    };
    if let Some(why) = failed {
        for (p, held) in before {
            let _ = match held {
                Some(bytes) => std::fs::write(&p, bytes),
                None => std::fs::remove_file(&p),
            };
        }
        return Err(format!("git shook install refused the monitor that copies the files of an instance: {why}"));
    }
    Ok(())
}

/// Whether the monitor is there and git asks it: `core.fsmonitor` names it, or names git-shook's
/// dispatcher with the monitor registered. A monitor whose engine is gone is not there.
pub(super) fn check(cfg: &Config) -> Result<(), String> {
    let engine = Engine::of(cfg)?;
    let common = common_dir(cfg)?;
    let at = common.join("hooks").join(NAME);
    let shook = common.join("shook");
    let dispatcher = shook.join("hooks").join(EVENT);
    let value = config(cfg, "core.fsmonitor")?;
    let current = setting_of(value.as_deref(), cfg, &at, &dispatcher);
    let live = std::fs::read_to_string(&at).is_ok_and(|t| current_or_live(&t, MARK, &engine, text));
    let missing = || {
        format!(
            "this worktree has no monitor that copies the files of an instance before git reads the work tree ({}); `cargo \
             mock data-refs instantiate` installs it",
            at.display()
        )
    };
    match current {
        Setting::Ours if live => Ok(()),
        Setting::Dispatcher if live => {
            let entry = shook.join("entries").join(EVENT).join(ENTRY);
            let runs = std::fs::read_to_string(&entry).is_ok_and(|t| t.lines().nth(1) == Some(&*at.to_string_lossy()));
            if runs && dispatcher.is_file() {
                Ok(())
            } else {
                Err(format!(
                    "core.fsmonitor names git-shook's dispatcher, and the monitor that copies the files of an instance before \
                     git reads the work tree is not registered with it ({}); `cargo mock data-refs instantiate` registers it",
                    entry.display()
                ))
            }
        },
        Setting::Daemon | Setting::Theirs(_) => Err(format!(
            "core.fsmonitor is set to {}, so git asks no monitor that copies the files of an instance before it reads the \
             work tree, and the plain way back from a commit from before the migration deletes an edit made there with no \
             copy of it; `cargo mock data-refs instantiate` runs both through git-shook",
            value.as_deref().unwrap_or_default()
        )),
        _ => Err(missing()),
    }
}

/// Says, for the monitor, whether the index of this worktree tracks a file of an instance: the file
/// [`TRACKED`] where it does, none where it does not. The file is kept in the cache before it is
/// removed.
pub(super) fn mark(cfg: &Config, tracked: bool) -> Result<(), String> {
    let file = PathBuf::from(git(cfg, &["rev-parse", "--path-format=absolute", "--git-path", TRACKED])?);
    match (tracked, file.exists()) {
        (true, false) => std::fs::write(&file, "the index of this worktree tracks a file of an instance\n")
            .map_err(|e| format!("writing {}: {e}", file.display())),
        (false, true) => {
            mockspace_store::kept::keep(&file).map_err(|e| format!("keeping a copy of {} before removing it: {e}", file.display()))?;
            std::fs::remove_file(&file).map_err(|e| format!("removing {}: {e}", file.display()))
        },
        _ => Ok(()),
    }
}
