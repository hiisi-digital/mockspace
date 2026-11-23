//! Every refusal of the setup that keeps git off the instances is made before anything is written:
//! a migration and an `instantiate` that stop on a hook of the person's own, a patterns file that is
//! not text, a cache no copy can be made in, or a monitor of the person's own with no git-shook to
//! run it beside the engine's, leave the work tree, the index, the config, the hooks, the references
//! and the remote as they were.

#[path = "common/ambient.rs"]
mod ambient;
#[path = "common/exec.rs"]
mod exec;
mod refs_e2e;

use std::path::{Path, PathBuf};

use refs_e2e::{Clone, Out, fixture, whole};

/// A refusal, as the person's clone is set up to meet it.
#[derive(Clone, Copy, Debug)]
enum Refusal {
    /// A `reference-transaction` hook of the person's own.
    TheirHook,
    /// A sparse-checkout file that is not text.
    PatternsNotText,
    /// A file where the directory of the copies goes.
    NoCache,
    /// A monitor of the person's own, and no git-shook on the `PATH`.
    TheirMonitor,
}

const REFUSALS: [Refusal; 4] = [Refusal::TheirHook, Refusal::PatternsNotText, Refusal::NoCache, Refusal::TheirMonitor];

impl Refusal {
    fn meet(self, c: &Clone) {
        let common = common(c);
        match self {
            Refusal::TheirHook => exec::write_executable(&common.join("hooks/reference-transaction"), "#!/bin/sh\n# their own\nexit 0\n"),
            Refusal::PatternsNotText => {
                std::fs::create_dir_all(common.join("info")).unwrap();
                std::fs::write(common.join("info/sparse-checkout"), b"/*\n\xff\xfe not text\n").unwrap();
            },
            Refusal::NoCache => {
                let _ = std::fs::remove_dir_all(c.w.kept_dir());
                std::fs::write(c.w.kept_dir(), b"not a directory\n").unwrap();
            },
            Refusal::TheirMonitor => {
                let theirs = c.w.base().join("their-monitor");
                exec::write_executable(&theirs, "#!/bin/sh\nprintf '/\\0'\n");
                c.git(&["config", "core.fsmonitor", theirs.to_str().unwrap()]);
            },
        }
    }

    /// What the refusal names.
    fn says(self) -> &'static str {
        match self {
            Refusal::TheirHook => "a hook of your own",
            Refusal::PatternsNotText => "cannot be read as text",
            Refusal::NoCache => "no copy of a file can be kept",
            Refusal::TheirMonitor => "git-shook",
        }
    }
}

fn common(c: &Clone) -> PathBuf {
    PathBuf::from(c.git(&["rev-parse", "--path-format=absolute", "--git-common-dir"]).trim())
}

/// The world's `PATH` with every directory holding a `git-shook` left out.
fn path_without_shook(c: &Clone) -> String {
    let rest: Vec<String> =
        std::env::var("PATH").unwrap().split(':').filter(|d| !Path::new(d).join("git-shook").exists()).map(str::to_owned).collect();
    format!("{}:{}", c.w.base().join("shims").display(), rest.join(":"))
}

fn run(c: &Clone, args: &[&str]) -> Out {
    let o = c
        .w
        .command(refs_e2e::world::BIN, &c.root)
        .env("PATH", path_without_shook(c))
        .arg("--dir")
        .arg(c.mock())
        .args(args)
        .output()
        .unwrap();
    Out { ok: o.status.success(), stdout: String::from_utf8_lossy(&o.stdout).into_owned(), stderr: String::from_utf8_lossy(&o.stderr).into_owned() }
}

/// Everything a refusal must leave as it was: the work tree, the references here and on the remote,
/// HEAD, the index, the config, the hooks, and the files of the git directory the setup writes.
#[derive(Debug, PartialEq)]
struct Held {
    disk:   whole::Disk,
    local:  whole::Refs,
    remote: whole::Refs,
    head:   (String, String),
    index:  Vec<String>,
    config: Vec<(String, String)>,
    files:  whole::Disk,
}

fn held(c: &Clone) -> Held {
    let w = whole::capture(c);
    let common = common(c);
    let mut files = whole::Disk::new();
    for dir in ["hooks", "info", "shook"] {
        if common.join(dir).exists() {
            files.extend(whole::disk(&common.join(dir)).into_iter().map(|(p, e)| ([dir.as_bytes(), b"/", &p].concat(), e)));
        }
    }
    for f in ["config", "mock-data-refs.json", "mock-data-refs.dirs", "mock-data-refs.tracked"] {
        if let Ok(bytes) = std::fs::read(common.join(f)) {
            files.insert(f.as_bytes().to_vec(), whole::Entry::File { exec: false, bytes });
        }
    }
    Held {
        disk: w.disk,
        local: w.local,
        remote: w.remote,
        head: w.git.head,
        index: w.git.index.iter().map(|(k, v)| format!("{k:?} {v:?}")).collect(),
        config: w.git.config,
        files,
    }
}

// Mutation: write the sparse switch, the monitor or the hooks before the refusal, or carry out a
// migration's seed, config and removals before its last step refuses. The clone is then half set
// up: a setting or a file written, the files untracked, a ref made, with the refusal said.
#[test]
fn a_migration_refused_on_the_setup_changes_nothing() {
    for refusal in REFUSALS {
        let w = refs_e2e::World::new();
        let c = w.first_clone("seed");
        fixture::build(&c);
        let _ = run(&c, &["status"]);
        refusal.meet(&c);
        let before = held(&c);
        let o = run(&c, &fixture::migrate_args("research", "research", &[]).iter().map(String::as_str).collect::<Vec<_>>());
        assert!(!o.ok && o.all().contains(refusal.says()), "{refusal:?}: refused, naming it: {}", o.all());
        assert!(o.all().contains("nothing was changed"), "{refusal:?}: {}", o.all());
        let after = held(&c);
        assert_eq!(after, before, "{refusal:?}: the refused migration changed nothing");
    }
}

// Mutation: as above, for `instantiate`: materialize the instances, or write the setting, before
// the refusal. The files of the refs then stand in a clone git is not kept off of.
#[test]
fn an_instantiate_refused_on_the_setup_changes_nothing() {
    let w = fixture::migrated_remote();
    for (n, refusal) in REFUSALS.into_iter().enumerate() {
        let c = w.clone_of(&format!("c{n}"));
        let _ = run(&c, &["status"]);
        refusal.meet(&c);
        let before = held(&c);
        let o = run(&c, &["data-refs", "instantiate"]);
        assert!(!o.ok && o.all().contains(refusal.says()), "{refusal:?}: refused, naming it: {}", o.all());
        assert!(o.all().contains("nothing was changed"), "{refusal:?}: {}", o.all());
        let after = held(&c);
        assert_eq!(after, before, "{refusal:?}: the refused instantiate changed nothing");
        if let Refusal::NoCache = refusal {
            std::fs::remove_file(w.kept_dir()).unwrap();
        }
    }
}
