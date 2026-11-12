//! The git setups a person keeps in a clone of their own beside the files, which a migration works
//! under and leaves as the person had them: a sparse checkout in cone mode, a `reference-transaction`
//! hook of their own, `core.hooksPath` at a directory of their own, git-shook dispatching the hooks,
//! and a bare clone whose branches are linked worktrees.

use std::path::{Path, PathBuf};

use super::world::{Clone, Out, World};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Setup {
    /// A sparse checkout in cone mode naming every directory the branch has.
    Cone,
    /// A `reference-transaction` hook of the person's own in the repository's hooks directory.
    OwnRefHook,
    /// `core.hooksPath` at a directory of the person's own, with a hook in it.
    HooksPath,
    /// git-shook holding `core.hooksPath`, installed once in the clone.
    Shook,
    /// A bare clone whose `dev` is checked out in a linked worktree, and a second branch in another.
    BareWorktrees,
}

pub const SETUPS: [Setup; 5] = [Setup::Cone, Setup::OwnRefHook, Setup::HooksPath, Setup::Shook, Setup::BareWorktrees];

/// The person's own hook, kept by every setup that has one.
pub const OWN_HOOK: &str = "#!/bin/sh\n# the person's own, not the engine's\nexit 0\n";

/// Where the `git-shook` a scenario uses is, or `None` where none is on the `PATH`.
pub fn shook_on_path(w: &World) -> Option<PathBuf> {
    let o = w.sh(&w.base(), "sh", &["-c", "command -v git-shook"]);
    o.ok.then(|| PathBuf::from(o.stdout.trim()))
}

impl Setup {
    /// The person's clone of the world's remote, with this setup made. For the bare clone, the
    /// linked worktree `dev` is checked out in.
    pub fn clone_in<'w>(&self, w: &'w World, name: &str) -> Clone<'w> {
        let c = if *self == Setup::BareWorktrees { w.bare_with_worktrees(name) } else { w.clone_of(name) };
        match self {
            Setup::Cone => {
                let dirs: Vec<String> = std::fs::read_dir(&c.root)
                    .unwrap()
                    .flatten()
                    .filter(|e| e.path().is_dir() && e.file_name() != ".git")
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect();
                let mut args = vec!["sparse-checkout", "set", "--cone"];
                args.extend(dirs.iter().map(String::as_str));
                c.git(&args);
            },
            Setup::OwnRefHook => {
                mockspace_manifest::write_executable(&c.hooks_dir().join("reference-transaction"), OWN_HOOK).unwrap();
            },
            Setup::HooksPath => {
                let own = w.base().join("own-hooks");
                mockspace_manifest::write_executable(&own.join("pre-commit"), OWN_HOOK).unwrap();
                c.git(&["config", "core.hooksPath", own.to_str().unwrap()]);
            },
            Setup::Shook => {
                // the project chains a tool of its own through git-shook, declared on its branch
                c.write("shook.toml", b"name = \"tidy\"\n\n[hooks.pre-commit]\nrun = [\"true\"]\n");
                c.commit_all("chore: chain the hooks through git-shook");
                let o = w.sh(&c.root, "git", &["shook", "install", "--yes"]);
                assert!(o.ok, "git shook install: {}", o.all());
            },
            Setup::BareWorktrees => {},
        }
        c
    }

    /// What the person does where a migration stops on this setup and says why: `Some` with the
    /// steps taken, where the run names what this setup is expected to stop it on, and `None` where
    /// the stop is not one this setup explains.
    pub fn way_on(&self, c: &Clone, out: &Out) -> Option<String> {
        match self {
            // the hook is never written over; the engine names it, and the person moves it aside
            Setup::OwnRefHook => {
                let hook = c.hooks_dir().join("reference-transaction");
                let said = out.all();
                if !(said.contains("reference-transaction") && said.contains("a hook of your own")) {
                    return None;
                }
                std::fs::rename(&hook, hook.with_extension("mine")).unwrap();
                Some(format!("moved {} aside", hook.display()))
            },
            _ => None,
        }
    }
}

impl World {
    /// A bare clone of the remote, `<name>.git`, with `dev` checked out in the linked worktree
    /// `<name>` and a branch `side` in `<name>-side`; the worktree `<name>` is the clone returned.
    pub fn bare_with_worktrees(&self, name: &str) -> Clone<'_> {
        let bare = format!("{name}.git");
        let o = self.sh(&self.base(), "git", &["clone", "-q", "--bare", self.remote.to_str().unwrap(), &bare]);
        assert!(o.ok, "clone --bare: {}", o.all());
        let git_dir = self.base().join(&bare);
        for (dir, branch) in [(name.to_string(), "dev".to_string()), (format!("{name}-side"), "side".to_string())] {
            let args: Vec<String> = if branch == "dev" {
                vec!["worktree".into(), "add".into(), "-q".into(), format!("../{dir}"), branch]
            } else {
                vec!["worktree".into(), "add".into(), "-q".into(), "-b".into(), branch, format!("../{dir}"), "dev".into()]
            };
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let o = self.sh(&git_dir, "git", &args);
            assert!(o.ok, "worktree add: {}", o.all());
        }
        let c = Clone { w: self, root: self.base().join(name) };
        c.configure();
        c
    }
}

impl Clone<'_> {
    /// The hooks directory of the repository, which a linked worktree shares with the others.
    pub fn hooks_dir(&self) -> PathBuf {
        let o = self.w.sh(&self.root, "git", &["rev-parse", "--path-format=absolute", "--git-common-dir"]);
        assert!(o.ok, "{}", o.all());
        Path::new(o.stdout.trim()).join("hooks")
    }
}
