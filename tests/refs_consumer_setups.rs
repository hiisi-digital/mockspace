//! Every migration of a consumer made in a clone the person has set up their own way: a sparse
//! checkout in cone mode, a `reference-transaction` hook of their own, `core.hooksPath` at a directory
//! of their own, git-shook dispatching the hooks, and a bare clone with linked worktrees. Each run is
//! checked over the whole tree and every reference; where the engine stops on the setup, it names
//! what stops it and the person's way on is taken; at the end the setup is the person's still, and a
//! reset or a checkout of a commit from before the migration and back keeps an edit of an instance
//! file.

#[path = "common/ambient.rs"]
mod ambient;
mod refs_e2e;

use refs_e2e::consumer;
use refs_e2e::setups::{self, OWN_HOOK, Setup};
use refs_e2e::whole;

fn strs(a: &[String]) -> Vec<&str> {
    a.iter().map(String::as_str).collect()
}

fn migrated_under(setup: Setup) {
    let s = consumer::ELSEWHERE;
    let w = consumer::remote(&s);
    assert!(
        setup != Setup::Shook || setups::shook_on_path(&w).is_some(),
        "git-shook is not on the PATH: put a `git-shook` that knows `git shook install --yes --only` (git-shook's `dev`, \
         built) first on the PATH, since the clone set up with it is a case this test owes"
    );
    let c = setup.clone_in(&w, "a");
    s.add_ignored_output(&c);
    // the engine sets the clone up on its first run, before any migration
    let _ = c.run(&["status"]);
    let before_any = c.git(&["rev-parse", "HEAD"]).trim().to_string();
    let mut said = String::new();
    let mut ways = Vec::new();
    for (k, args) in s.calls().iter().enumerate() {
        let args = strs(args);
        let mut before = whole::capture(&c);
        let mut run = c.run(&args);
        if !run.ok {
            if let Err(e) = whole::kept(&c, &before, &whole::capture(&c), &s.allowed(k)) {
                panic!("{setup:?} {args:?}: the stopped run lost something:\n{e}\n{}", run.all());
            }
            let Some(did) = setup.way_on(&c, &run) else {
                panic!("{setup:?} {args:?}: stopped, on nothing this setup explains:\n{}", run.all());
            };
            ways.push(did);
            before = whole::capture(&c);
            run = c.run(&args);
            assert!(run.ok, "{setup:?} {args:?}: not finished once the person took the way on:\n{}", run.all());
        }
        said.push_str(&run.all());
        if let Err(e) = whole::kept(&c, &before, &whole::capture(&c), &s.allowed(k)) {
            panic!("{setup:?} {args:?}: the run lost something:\n{e}\n{}", run.all());
        }
        c.commit_all(&format!("chore: migration {k}"));
    }
    // the rewritten templates are on their ref's instance, and go to the ref as the run says
    c.ok(&["data-refs", "commit"]);
    c.ok(&["data-refs", "verify"]);

    // the setup is the person's still
    match setup {
        Setup::Cone => {
            assert!(said.contains("cone"), "the switch out of cone mode is said:\n{said}");
            let cone = c.git_out(&["config", "--type=bool", "core.sparseCheckoutCone"]).stdout.trim().to_string();
            assert_ne!(cone, "true", "the block needs the full pattern mode");
        },
        Setup::OwnRefHook => {
            assert_eq!(ways.len(), 1, "the engine stopped once on the person's hook: {ways:?}");
            let moved = c.hooks_dir().join("reference-transaction.mine");
            assert_eq!(std::fs::read_to_string(&moved).unwrap(), OWN_HOOK, "the person's hook, moved aside by them, is as it was");
        },
        Setup::HooksPath => {
            let own = w.base().join("own-hooks");
            let saved = c.git_out(&["config", "mockspace.previousHooksPath"]).stdout.trim().to_string();
            assert_eq!(saved, own.display().to_string(), "the gate saved the person's `core.hooksPath`");
            assert_eq!(std::fs::read_to_string(own.join("pre-commit")).unwrap(), OWN_HOOK, "the person's hook is as it was");
            c.ok(&["deactivate"]);
            let back = c.git(&["config", "core.hooksPath"]).trim().to_string();
            assert_eq!(back, own.display().to_string(), "`deactivate` puts the person's `core.hooksPath` back");
            // there the hook that copies an edit is not reached, and `verify` says so where nothing
            // takes the path again first
            let off = c.w.sh(&c.root, "sh", &["-c", "MOCKSPACE_NO_AUTO_ACTIVATE=1 mock data-refs verify"]);
            assert!(!off.ok && off.all().contains("core.hooksPath"), "`verify` names a `core.hooksPath` that does not reach the hook: {}", off.all());
            c.ok(&["data-refs", "verify"]);
            let gate = c.git(&["config", "core.hooksPath"]).trim().to_string();
            assert_ne!(gate, own.display().to_string(), "the next engine run takes the path again");
        },
        Setup::Shook => {
            let path = c.git(&["config", "core.hooksPath"]).trim().to_string();
            assert!(path.ends_with("shook/hooks"), "git-shook holds `core.hooksPath` still: {path}");
        },
        Setup::BareWorktrees => {
            let side = c.at(w.base().join("a-side"));
            assert_eq!(side.git(&["rev-parse", "--is-bare-repository"]).trim(), "false", "the other worktree is no bare one");
            assert_eq!(side.git(&["status", "--porcelain", "--untracked-files=no"]), "", "the other worktree is as it was");
            let bare = w.sh(&w.base().join("a.git"), "git", &["rev-parse", "--is-bare-repository"]);
            assert_eq!(bare.stdout.trim(), "true", "the clone itself is bare still: {}", bare.all());
            // a linked worktree that left `dev` moves it as it did before the migration, by a fetch
            // into it and by `branch -f`, and goes back to it: the sparse switch sits in the common
            // config, and `core.bare` stays where every worktree reads it
            // the remote holds what the migration made, so the fetch into `dev` has nothing to rewind
            c.git(&["push", "-q", "--no-verify", "origin", "dev"]);
            let tip = c.git(&["rev-parse", "HEAD"]).trim().to_string();
            c.git(&["checkout", "-q", "--detach"]);
            let mut refused = Vec::new();
            for args in [&["fetch", "origin", "dev:dev"][..], &["branch", "-f", "dev", &tip], &["checkout", "-q", "dev"]] {
                let o = c.git_out(args);
                if !o.ok {
                    refused.push(format!("{args:?}: {}", o.all().trim()));
                }
            }
            assert!(refused.is_empty(), "a linked worktree of the bare clone moves and takes `dev` again:\n{}", refused.join("\n"));
            // the worktree with patterns an earlier sparse checkout left keeps every file of its
            // branch once the shared switch reads them, and the file it held is in the cache
            let stale = c.at(w.base().join("a-stale"));
            stale.git(&["checkout", "-q", "-b", "stale-2"]);
            stale.git(&["checkout", "-q", "stale"]);
            let skipped: Vec<String> =
                stale.git(&["ls-files", "-t"]).lines().filter(|l| l.starts_with("S ")).map(str::to_owned).collect();
            assert!(skipped.is_empty(), "the stale patterns hid the branch's files: {skipped:?}");
            let tracked = stale.git(&["ls-files", "-z"]);
            let gone: Vec<&str> = tracked.split('\0').filter(|p| !p.is_empty() && !stale.exists(p)).collect();
            assert!(gone.is_empty(), "files of the branch are gone from the worktree: {gone:?}");
            let file = w.base().join("a.git/worktrees/a-stale/info/sparse-checkout");
            assert!(w.kept_copies_of(&file).iter().any(|k| std::fs::read_to_string(k).ok().as_deref() == Some(setups::STALE_PATTERNS)), "a copy of {} is in the cache", file.display());
        },
    }

    // an edit of an instance file is kept by both ways to a commit from before the migration and
    // back that the engine covers: a reset there and back, where the hook copies on the move of
    // `ORIG_HEAD` there, and a checkout there with an engine command run at it and a forced checkout
    // back; on disk, or by the way back the engine says
    let rel = "20261001_alpha/NOTES.md";
    let file = s.at(&format!("research/{rel}"));
    let tip = c.git(&["rev-parse", "HEAD"]).trim().to_string();
    for (n, by_reset) in [(0, true), (1, false)] {
        let mut edited = c.read(&file);
        edited.extend_from_slice(format!("edited in the instance, {n}\n").as_bytes());
        c.write(&file, &edited);
        let what = format!("{setup:?}, {}", if by_reset { "a reset there and back" } else { "a checkout there and a forced one back" });
        if by_reset {
            c.git(&["reset", "-q", "--hard", &before_any]);
            c.git(&["reset", "-q", "--hard", &tip]);
        } else {
            c.git(&["checkout", "-q", &before_any]);
            let _ = c.run(&["data-refs", "status"]);
            let back = c.git_out(&["checkout", "-q", "-f", "dev"]);
            // a linked worktree of the bare clone goes back to `dev` as before the migration
            assert!(back.ok, "{what}: the linked worktree goes back to `dev`: {}", back.all());
        }
        refs_e2e::daily::edit_back_after_forced_way_back(&c, "research", rel, &file, &edited, &what);
    }
}

#[test]
fn every_migration_keeps_a_sparse_checkout_in_cone_mode() {
    migrated_under(Setup::Cone);
}

#[test]
fn every_migration_keeps_a_reference_transaction_hook_of_the_persons_own() {
    migrated_under(Setup::OwnRefHook);
}

#[test]
fn every_migration_keeps_core_hooks_path_at_a_directory_of_the_persons_own() {
    migrated_under(Setup::HooksPath);
}

#[test]
fn every_migration_keeps_git_shook_dispatching_the_hooks() {
    migrated_under(Setup::Shook);
}

#[test]
fn every_migration_keeps_a_bare_clone_with_linked_worktrees() {
    migrated_under(Setup::BareWorktrees);
}
