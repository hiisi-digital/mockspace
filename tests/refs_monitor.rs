//! The monitor git asks before it reads the work tree, and the program the hooks and the monitor run:
//! where it is set, how it answers git, how it runs beside a monitor of the person's own through
//! git-shook, and the engine it runs, at a commit whose pin names an engine with no `snapshot` and
//! where the engine that installed it is gone.

#[path = "common/ambient.rs"]
mod ambient;
#[path = "common/exec.rs"]
mod exec;
mod refs_e2e;

use std::path::{Path, PathBuf};

use fixture::NOTES;
use refs_e2e::{Clone, Out, fixture};

fn common(c: &Clone) -> PathBuf {
    PathBuf::from(c.git(&["rev-parse", "--path-format=absolute", "--git-common-dir"]).trim())
}

fn monitor(c: &Clone) -> PathBuf {
    common(c).join("hooks/mockspace-fsmonitor")
}

fn dispatcher(c: &Clone) -> PathBuf {
    common(c).join("shook/hooks/fsmonitor-watchman")
}

/// The commit just before the research moved to its ref.
fn before_the_migration(c: &Clone) -> String {
    let migration = c.git(&["log", "-1", "--format=%H", "--", "mock/research"]);
    c.git(&["rev-parse", &format!("{}~1", migration.trim())]).trim().to_string()
}

/// The engine at `engine`, run in `c` as the world runs it, with `path` for the `PATH` where given.
fn run_engine(c: &Clone, engine: &Path, path: Option<&str>, args: &[&str]) -> Out {
    let mut cmd = c.w.command(engine.to_str().unwrap(), &c.root);
    if let Some(p) = path {
        cmd.env("PATH", p);
    }
    let o = cmd.arg("--dir").arg(c.mock()).args(args).output().unwrap();
    Out { ok: o.status.success(), stdout: String::from_utf8_lossy(&o.stdout).into_owned(), stderr: String::from_utf8_lossy(&o.stderr).into_owned() }
}

/// The world's `PATH` with every directory holding a `git-shook` left out.
fn path_without_shook(c: &Clone) -> String {
    let rest: Vec<String> = std::env::var("PATH")
        .unwrap()
        .split(':')
        .filter(|d| !Path::new(d).join("git-shook").exists())
        .map(str::to_owned)
        .collect();
    format!("{}:{}", c.w.base().join("shims").display(), rest.join(":"))
}

/// A monitor of the person's own: it says it ran, in `log`, and answers git that any file may have
/// changed, by the version git asks for.
fn their_monitor(c: &Clone, log: &Path) -> PathBuf {
    let at = c.w.base().join("their monitor");
    exec::write_executable(
        &at,
        format!("#!/bin/sh\necho \"ran $*\" >> '{}'\nif [ \"$1\" = 1 ]; then printf '/\\0'; else printf 'theirs\\0/\\0'; fi\n", log.display()),
    );
    at
}

/// The paths git's trace says the monitor named as changed.
fn named_by_the_monitor(c: &Clone) -> Vec<String> {
    let o = c.w.command("git", &c.root).env("GIT_TRACE_FSMONITOR", "1").args(["status", "--porcelain"]).output().unwrap();
    String::from_utf8_lossy(&o.stderr)
        .lines()
        .filter_map(|l| l.split("fsmonitor_refresh_callback '").nth(1))
        .filter_map(|l| l.split('\'').next())
        .map(str::to_owned)
        .collect()
}

/// After the plain way back, the edit `text` of the notes is on disk or comes back as the engine says.
fn kept_on_the_way_back(c: &Clone, text: &[u8], what: &str) {
    refs_e2e::daily::edit_back_after_forced_way_back(c, "research", "20261001_alpha/NOTES.md", NOTES, text, what);
}

// Mutation: leave `verify` blind to the monitor, or `instantiate` not writing it again; an edit
// made at a commit from before the migration is then lost on the plain way back with nothing said.
#[test]
fn verify_names_a_worktree_with_no_monitor_and_instantiate_repairs_it() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    a.ok(&["data-refs", "verify"]);
    let set = a.git(&["config", "core.fsmonitor"]);
    assert_eq!(set.trim(), format!("'{}'", monitor(&a).display()), "the setting names the monitor, quoted for the shell");
    for broken in ["unset", "removed"] {
        if broken == "unset" {
            a.git(&["config", "--unset", "core.fsmonitor"]);
        } else {
            std::fs::remove_file(monitor(&a)).unwrap();
        }
        let o = a.fails(&["data-refs", "verify"]);
        assert!(o.all().contains("monitor") && o.all().contains("instantiate"), "{broken}: {}", o.all());
        let _ = a.run(&["data-refs", "instantiate"]);
        a.ok(&["data-refs", "verify"]);
    }
}

// Mutation: answer `<token>\0/\0` whatever version git asks for. Under version 1 git reads the token
// as a path the monitor named; and a monitor answering no `/` hides a change from `status`.
#[test]
fn the_monitor_answers_by_the_protocol_version_git_asks_for() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    let file = a.git(&["ls-files"]).lines().next().unwrap().to_string();
    let was = a.read(&file);
    for version in ["1", "2"] {
        a.git(&["config", "core.fsmonitorHookVersion", version]);
        a.write(&file, format!("changed on disk under version {version}\n").as_bytes());
        let s = a.git(&["status", "--porcelain"]);
        assert!(s.contains(&file), "version {version}: git sees the change through the monitor: {s}");
        let named = named_by_the_monitor(&a);
        assert!(named.iter().all(|p| p == "/"), "version {version}: the monitor names no path but every path: {named:?}");
        a.write(&file, &was);
        let s = a.git(&["status", "--porcelain"]);
        assert!(!s.contains(&file), "version {version}: git sees the file back as it was: {s}");
    }
}

// Mutation: have the monitor run the copy whatever the file says, or answer git with no `/`. The
// copy then costs every git command a run of the engine, or git takes a file changed on disk for an
// unchanged one. The monitor copies only while the index tracks a file of an instance, and git
// sees every change with it as without it.
#[test]
fn the_monitor_copies_only_while_the_index_tracks_a_file_of_an_instance_and_hides_no_change() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    let tracked = || PathBuf::from(a.git(&["rev-parse", "--path-format=absolute", "--git-path", "mock-data-refs.tracked"]).trim());
    let copies = || a.git_out(&["rev-list", "--count", "refs/mock-backup/research"]).stdout.trim().to_string();
    a.write(NOTES, b"edited on dev\n");
    let _ = a.run(&["data-refs", "status"]);
    let before = copies();
    for _ in 0 .. 3 {
        a.git(&["status", "--short"]);
    }
    assert_eq!(copies(), before, "a git command on dev copies nothing");
    assert!(!tracked().exists(), "on dev the index tracks no file of an instance");
    let file = a.git(&["ls-files"]).lines().next().unwrap().to_string();
    let was = a.read(&file);
    a.write(&file, b"changed on disk\n");
    let s = a.git(&["status", "--porcelain"]);
    assert!(s.contains(&file), "git sees the change through the monitor: {s}");
    let pre = before_the_migration(&a);
    a.write(&file, &was);
    let s = a.git(&["status", "--porcelain"]);
    assert!(!s.contains(&file), "git sees the file back as it was through the monitor: {s}");
    a.git(&["checkout", "-q", &pre]);
    assert!(tracked().exists(), "at a commit from before the migration the index tracks the files of the instance");
    a.write(NOTES, b"edited at the old commit\n");
    a.git(&["status", "--short"]);
    let held = a.git(&["rev-list", "refs/mock-backup/research"]);
    let kept = held.lines().any(|c| {
        a.git(&["ls-tree", "-r", "--name-only", c]).lines().filter(|p| p.ends_with("NOTES.md")).any(|p| {
            a.git_out(&["show", &format!("{c}:{p}")]).stdout == "edited at the old commit\n"
        })
    });
    assert!(kept, "a git command there copies the edit made there");
    a.git(&["checkout", "-q", "dev"]);
    assert!(!tracked().exists(), "back on dev the file is gone");
}

// Mutation: write the setting over a monitor of the person's own where git-shook cannot run, or
// write the config or the hooks before the refusal. Their monitor is then gone with no word, or the
// refusal leaves the clone half set up.
#[test]
fn a_monitor_of_the_persons_own_with_no_git_shook_to_run_it_is_refused_and_nothing_changes() {
    let w = fixture::migrated_remote();
    let a = w.clone_of("a");
    let theirs = their_monitor(&a, &w.base().join("their.log"));
    let setting = format!("'{}'", theirs.display());
    a.git(&["config", "core.fsmonitor", &setting]);
    // the engine sets the clone up on its first run, the gate and the drivers, before any data ref
    let _ = run_engine(&a, Path::new(refs_e2e::world::BIN), Some(&path_without_shook(&a)), &["status"]);
    let before = refs_e2e::whole::capture(&a);
    let o = run_engine(&a, Path::new(refs_e2e::world::BIN), Some(&path_without_shook(&a)), &["data-refs", "instantiate"]);
    assert!(!o.ok && o.all().contains("git-shook") && o.all().contains("nothing was changed"), "{}", o.all());
    let after = refs_e2e::whole::capture(&a);
    assert_eq!(a.git(&["config", "core.fsmonitor"]).trim(), setting, "the setting is as it was");
    assert_eq!(after.git.config, before.git.config, "the config is as it was");
    assert_eq!(after.git.index, before.git.index, "the index is as it was");
    assert_eq!(after.git.hooks, before.git.hooks, "the hooks are as they were");
    assert_eq!(after.disk, before.disk, "the work tree is as it was");
    // in a clone set up already, `verify` names the setting that leaves the monitor out
    let b = fixture::seat(&w, "b");
    b.git(&["config", "core.fsmonitor", &setting]);
    let v = run_engine(&b, Path::new(refs_e2e::world::BIN), Some(&path_without_shook(&b)), &["data-refs", "verify"]);
    assert!(!v.ok && v.all().contains("core.fsmonitor is set to"), "{}", v.all());
}

// Mutation: refuse `core.fsmonitor=true`, or take it with nothing said. git's own daemon answers no
// program a dispatcher could run, so where git-shook does not hold the hooks the monitor takes the
// setting, says so, and the config that held it is in the cache.
#[test]
fn gits_own_daemon_gives_the_setting_to_the_monitor_and_that_is_said() {
    let w = fixture::migrated_remote();
    let a = w.clone_of("a");
    a.git(&["config", "core.fsmonitor", "true"]);
    let o = a.ok(&["data-refs", "instantiate"]);
    assert!(o.all().contains("git's own daemon"), "{}", o.all());
    assert_eq!(a.git(&["config", "core.fsmonitor"]).trim(), format!("'{}'", monitor(&a).display()));
    let config = common(&a).join("config");
    assert!(
        w.kept_copies_of(&config).iter().any(|k| std::fs::read_to_string(k).unwrap().contains("fsmonitor = true")),
        "the config that held the setting is in the cache"
    );
    a.ok(&["data-refs", "verify"]);
}

// Mutation: refuse a monitor of the person's own where git-shook runs, register theirs as an entry
// that does not answer, or leave the gate out of git-shook once it takes the hooks. Their monitor then
// answers git no more, or git takes no answer, or no hook copies an edit before git writes over it.
#[test]
fn a_monitor_of_the_persons_own_runs_beside_the_monitor_through_git_shook_and_answers_git() {
    let w = fixture::migrated_remote();
    let a = w.clone_of("a");
    assert!(
        refs_e2e::setups::shook_on_path(&w).is_some(),
        "git-shook is not on the PATH: put git-shook's `dev` script first on the PATH, since this case is owed"
    );
    let log = w.base().join("their.log");
    let theirs = their_monitor(&a, &log);
    a.git(&["config", "core.fsmonitor", &format!("'{}'", theirs.display())]);
    let _ = a.run(&["status"]);
    let o = a.ok(&["data-refs", "instantiate"]);
    assert!(o.all().contains("git-shook runs it now"), "the take-over is said: {}", o.all());
    let hooks_path = a.git(&["config", "core.hooksPath"]);
    assert!(hooks_path.trim().ends_with("shook/hooks"), "git-shook holds the hooks: {hooks_path}");
    assert_eq!(a.git(&["config", "core.fsmonitor"]).trim(), format!("'{}'", dispatcher(&a).display()), "git asks the dispatcher");
    let listed = w.sh(&a.root, "git", &["shook", "list"]);
    for entry in ["0-mockspace-data-refs", "0-mockspace-kept-fsmonitor", "00-mockspace"] {
        assert!(listed.stdout.contains(entry), "{entry} is registered: {}", listed.all());
    }
    a.ok(&["data-refs", "verify"]);
    // their monitor answers git, under either version, and git sees every change
    let file = a.git(&["ls-files"]).lines().next().unwrap().to_string();
    let was = a.read(&file);
    for version in ["1", "2"] {
        a.git(&["config", "core.fsmonitorHookVersion", version]);
        let ran = std::fs::read_to_string(&log).unwrap_or_default().lines().count();
        a.write(&file, format!("changed under version {version}\n").as_bytes());
        let s = a.git(&["status", "--porcelain"]);
        assert!(s.contains(&file), "version {version}: git sees the change: {s}");
        let now = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(now.lines().count() > ran && now.lines().last().unwrap().starts_with(&format!("ran {version}")), "version {version}: their monitor ran: {now}");
        assert!(named_by_the_monitor(&a).iter().all(|p| p == "/"), "version {version}: their answer reached git");
        a.write(&file, &was);
    }
    // and the edit made at a commit from before the migration is kept on the plain way back
    a.write(NOTES, b"edited on dev\n");
    a.git(&["checkout", "-q", &before_the_migration(&a)]);
    a.write(NOTES, b"edited at the old commit, git-shook dispatching\n");
    let back = a.git_out(&["checkout", "-q", "dev"]);
    assert!(back.ok, "{}", back.all());
    kept_on_the_way_back(&a, b"edited at the old commit, git-shook dispatching\n", "through git-shook");
}

/// Makes the world's launcher an engine from before data refs wherever the commit at the top of the
/// worktree declares no `[[data_ref]]`, as the launcher reads the pin of the commit there: it knows
/// no `data-refs` and says so.
fn an_engine_with_no_snapshot_at_old_commits(w: &refs_e2e::World) {
    let shims = w.base().join("shims");
    let script = "#!/bin/sh\ntop=$(git rev-parse --show-toplevel)\nif ! grep -q '^\\[\\[data_ref\\]\\]' \"$top/mock/mockspace.toml\" 2>/dev/null; then\n  case \" $* \" in *\" data-refs \"*) echo \"error: unknown subcommand data-refs (the engine this commit pins)\" >&2; exit 2 ;; esac\nfi\n";
    exec::write_executable(&shims.join("mock"), format!("{script}exec \"$MOCKSPACE_BIN\" --dir \"$top/mock\" \"$@\"\n"));
    exec::write_executable(&shims.join("cargo-mock"), format!("{script}exec \"$MOCKSPACE_BIN\" --dir \"$top/mock\" \"$@\"\n"));
}

// Mutation: run the launcher on the `PATH` from the hooks or the monitor. At a commit from before
// the migration it runs the engine that commit pins, which has no `snapshot`, so neither the hook on
// the way there nor the monitor on the way back copies, and the edit made there is lost.
#[test]
fn the_hooks_and_the_monitor_run_their_engine_where_the_old_commit_pins_one_with_no_snapshot() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    an_engine_with_no_snapshot_at_old_commits(&w);
    a.write(NOTES, b"edited on dev\n");
    let pre = before_the_migration(&a);
    a.git(&["checkout", "-q", &pre]);
    // the control: the launcher there is the engine with no `snapshot`
    let old = w.sh(&a.root, "sh", &["-c", "mock data-refs snapshot"]);
    assert!(!old.ok && old.all().contains("unknown subcommand"), "{}", old.all());
    a.write(NOTES, b"edited at the old commit, an old engine pinned\n");
    let back = a.git_out(&["checkout", "-q", "dev"]);
    assert!(back.ok, "{}", back.all());
    assert!(!back.all().contains("unknown subcommand"), "nothing ran the pinned engine: {}", back.all());
    kept_on_the_way_back(&a, b"edited at the old commit, an old engine pinned\n", "an old engine pinned there");
}

// Mutation: fail the git command, or copy nothing, where the engine that installed the hooks is
// gone; or let `verify` pass over a hook whose engine is gone. The edit made at a commit from before
// the migration is then lost, the person's command stops, or nothing says the hooks need writing
// again.
#[test]
fn where_the_engine_that_installed_the_hooks_is_gone_the_directories_are_copied_and_git_goes_on() {
    let w = fixture::migrated_remote();
    let a = w.clone_of("a");
    let gone = w.base().join("cache-cleaned");
    std::fs::create_dir_all(&gone).unwrap();
    let engine = gone.join("mockspace");
    exec::write_executable(&engine, std::fs::read(refs_e2e::world::BIN).unwrap());
    let o = run_engine(&a, &engine, None, &["data-refs", "instantiate"]);
    assert!(o.ok, "{}", o.all());
    let hook = std::fs::read_to_string(common(&a).join("hooks/post-checkout")).unwrap();
    assert!(hook.contains(&engine.display().to_string()), "the hook runs the engine that installed it");
    std::fs::remove_dir_all(&gone).unwrap();
    a.write(NOTES, b"edited on dev\n");
    let pre = before_the_migration(&a);
    let there = a.git_out(&["checkout", "-q", &pre]);
    assert!(there.ok && there.all().contains("is gone"), "the way there goes, and says the engine is gone: {}", there.all());
    a.write(NOTES, b"edited at the old commit, the engine gone\n");
    let back = a.git_out(&["checkout", "-q", "dev"]);
    assert!(back.ok, "the way back goes: {}", back.all());
    let copies = w.kept_copies_of(&a.path(NOTES));
    assert!(
        copies.iter().any(|k| std::fs::read(k).unwrap() == b"edited at the old commit, the engine gone\n"),
        "the cache holds the edit made at the old commit: {copies:?}"
    );
    // `verify` names the hooks whose engine is gone, and `instantiate` writes them for this one;
    // the files git took are written back first, so the look at the instance passes
    a.ok(&["data-refs", "restore"]);
    let v = a.fails(&["data-refs", "verify"]);
    assert!(v.all().contains("hook") && v.all().contains("instantiate"), "{}", v.all());
    // the instances are there already, which `instantiate` says, and it writes the hooks whatever
    let _ = a.run(&["data-refs", "instantiate"]);
    let v = a.run(&["data-refs", "verify"]);
    assert!(!v.all().contains("hook") && !v.all().contains("monitor"), "the hooks and the monitor are this engine's: {}", v.all());
    let hook = std::fs::read_to_string(common(&a).join("hooks/post-checkout")).unwrap();
    assert!(!hook.contains(&engine.display().to_string()), "the hook runs the engine there is now");
}

// Mutation: register the person's command as the entry that answers where git-shook has one that
// answers already. git-shook then refuses the install part way, or git reads two answers as one.
#[test]
fn a_monitor_of_the_persons_own_beside_an_entry_git_shook_has_answering_is_refused_and_nothing_changes() {
    let w = fixture::migrated_remote();
    let a = w.clone_of("a");
    assert!(refs_e2e::setups::shook_on_path(&w).is_some(), "git-shook is not on the PATH, and this case is owed");
    let log = w.base().join("their.log");
    let theirs = their_monitor(&a, &log);
    a.write("shook.toml", format!("name = \"tidy\"\n\n[hooks.fsmonitor-watchman]\nanswers = true\nrun = [\"{}\"]\n", theirs.display()).as_bytes());
    a.commit_all("chore: their monitor through git-shook");
    let o = w.sh(&a.root, "git", &["shook", "install", "--yes"]);
    assert!(o.ok, "{}", o.all());
    // a second monitor of theirs, which `core.fsmonitor` names instead of the dispatcher
    let other = w.base().join("another-monitor");
    exec::write_executable(&other, "#!/bin/sh\nprintf '/\\0'\n");
    a.git(&["config", "core.fsmonitor", other.to_str().unwrap()]);
    let _ = a.run(&["status"]);
    let config = a.git(&["config", "--list", "--local"]);
    let disk = refs_e2e::whole::disk(&a.root);
    let o = a.fails(&["data-refs", "instantiate"]);
    assert!(o.all().contains("already has `tidy` answering") && o.all().contains("nothing was changed"), "{}", o.all());
    assert_eq!(a.git(&["config", "--list", "--local"]), config, "the config is as it was");
    assert_eq!(refs_e2e::whole::disk(&a.root), disk, "the work tree is as it was");
}
