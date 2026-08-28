//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

/// Check if every crate in the workspace has been nuked.
pub(crate) fn detect_nuked_workspace(cfg: &Config) -> bool {
    // Every source directory. Answering this from one group would report a
    // fully nuked workspace while other groups still held their source, and
    // this is the check other behaviour keys off.
    let crate_dirs = crate::parse::package_dirs_in(&cfg.src_dirs);

    if crate_dirs.is_empty() {
        return false;
    }

    crate_dirs.iter().all(|dir| {
        let librs = dir.join("src/lib.rs");
        fs::read_to_string(&librs)
            .map(|s| s.contains(&cfg.nuke_marker))
            .unwrap_or(false)
    })
}

/// Whether everything in `repo` is already in git, so a deletion can be undone.
///
/// `git status --porcelain` is empty exactly when nothing is modified, staged
/// or untracked. An untracked file counts: it is the one thing git cannot give
/// back, and a source file somebody wrote and has not committed is precisely
/// what this is protecting.
///
/// A directory that is not a repository at all cannot be recovered from, so it
/// is refused rather than treated as clean.
pub(crate) fn tree_is_recoverable(repo: &Path) -> Result<(), String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["status", "--porcelain"])
        .output()
        .map_err(|e| format!("git could not be run: {e}"))?;
    if !out.status.success() {
        return Err(format!("{} is not a git repository", repo.display()));
    }
    let dirty = String::from_utf8_lossy(&out.stdout);
    if dirty.trim().is_empty() {
        return Ok(());
    }
    let lines: Vec<&str> = dirty.lines().take(5).collect();
    Err(format!(
        "the tree has {} path(s) git does not hold, starting with:\n  {}",
        dirty.lines().count(),
        lines.join("\n  ")
    ))
}

/// Which tier a nuke takes down.
///
/// The order is the mutation order in the canon, design, code chain: a design
/// may only change once the code beneath it is gone, so nuking the design tier
/// takes the source with it. There is no tier that takes designs and leaves
/// source standing, because that state is exactly what the chain forbids.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum NukeTier {
    /// Source only. The designs stay and the crates are rewritten from them.
    Src,
    /// Designs and the source under them. The designs are rewritten from the
    /// canon, and the source from the designs that result.
    Docs,
}

impl NukeTier {
    pub(crate) fn parse(word: &str) -> Result<Self, String> {
        match word {
            "src" | "source" => Ok(Self::Src),
            "doc" | "docs" | "design" | "designs" => Ok(Self::Docs),
            other => {
                Err(format!(
                    "no tier named `{other}`. It is `src` for the source alone, or \
                 `docs` for the designs and the source under them."
                ))
            },
        }
    }
}

/// What a command line asked a nuke to do.
#[derive(Debug)]
pub(crate) struct NukeRequest {
    pub(crate) tier: NukeTier,
    /// Whether to put the listing to somebody before acting on it.
    pub(crate) ask:  bool,
}

/// Read a nuke out of the arguments, or `None` when none was asked for.
///
/// `--nuke` on its own is a complete request. It used to be refused unless a
/// second flag followed it, which bought nothing: a second flag proves somebody
/// typed a second flag, and the thing worth being sure of is what is about to
/// go. So the listing is the safeguard now and the question is asked from it.
/// `--y` answers in advance, for the script and for the second time you run it.
pub(crate) fn read_request(args: &[String]) -> Option<Result<NukeRequest, String>> {
    let word = args
        .iter()
        .find(|a| a.as_str() == "--nuke" || a.starts_with("--nuke="))?;

    let tier = match word.split_once('=') {
        None => NukeTier::Src,
        Some((_, w)) => {
            match NukeTier::parse(w) {
                Ok(t) => t,
                Err(why) => return Some(Err(why)),
            }
        },
    };
    let ask = !args.iter().any(|a| a == "--y" || a == "--i-mean-it");
    Some(Ok(NukeRequest {
        tier,
        ask,
    }))
}
/// One crate the nuke reaches, and every file in it that goes.
pub(crate) struct NukedCrate {
    pub(crate) name:    String,
    /// Deleted outright.
    pub(crate) files:   Vec<PathBuf>,
    /// Overwritten with a stub rather than deleted, so the crate still builds
    /// far enough to say what it is.
    pub(crate) stub:    PathBuf,
    pub(crate) is_proc: bool,
}

/// Everything a nuke would take, gathered before any of it is taken.
///
/// The plan is the list and the list is what gets deleted. Enumerating twice,
/// once to show somebody and once to act, is how a listing comes to disagree
/// with what happens, and the direction it disagrees in is the one that matters.
pub(crate) struct NukePlan {
    pub(crate) tier:    NukeTier,
    pub(crate) crates:  Vec<NukedCrate>,
    /// Design templates, empty at [`NukeTier::Src`].
    pub(crate) docs:    Vec<PathBuf>,
    /// Paths the walk found and the guards refused, with why, kept so the listing
    /// can report them rather than dropping them in silence. A plan that quietly
    /// shrank is the same defect as a listing that understates.
    ///
    /// The reason travels with the path because the two cases read completely
    /// differently to somebody deciding: a path outside the repository says the
    /// configuration is wrong, and a symlink says this one file is not what it
    /// appears to be.
    pub(crate) escaped: Vec<(PathBuf, Refused)>,
}

/// Why a path did not make it into the plan.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Refused {
    /// It resolves outside the repository the clean-tree check looked at.
    Outside,
    /// It is a symlink, so writing or deleting it acts on something else.
    Link,
    /// Its crate was dropped because the crate's stub was refused.
    ItsCrateWent,
}

impl Refused {
    fn why(self) -> &'static str {
        match self {
            Self::Outside => "outside the repository",
            Self::Link => "a symlink, so it names something else",
            Self::ItsCrateWent => "its crate's stub was refused",
        }
    }
}

impl NukePlan {
    pub(crate) fn is_empty(&self) -> bool {
        self.crates.is_empty() && self.docs.is_empty()
    }

    /// Nothing survived the containment check, and something was refused by it.
    ///
    /// Told apart from an ordinary empty plan because the two want opposite
    /// answers: nothing to take is a success, and everything being outside the
    /// repository is a configuration that would have done damage.
    pub(crate) fn all_escaped(&self) -> bool {
        self.is_empty() && !self.escaped.is_empty()
    }

    /// What `apply` will report having taken, counted the same way it counts.
    ///
    /// The stub counts only where one is already there, because that is what
    /// `apply` does: on a crate with a `main.rs` and no `lib.rs` the two used to
    /// disagree by one, and a listing that disagrees with the deletion is the
    /// one thing this whole shape exists to prevent.
    pub(crate) fn file_count(&self) -> usize {
        self.crates
            .iter()
            .map(|c| c.files.len() + usize::from(c.stub.exists()))
            .sum::<usize>()
            + self.docs.len()
    }

    /// Everything, by name, on the way to asking whether to do it.
    ///
    /// Named in full rather than counted. A count is a claim somebody has to
    /// take on trust at the moment they can least afford to, and the whole
    /// reason this is interactive is that the answer depends on what is in the
    /// list.
    pub(crate) fn describe(&self, root: &Path) {
        let rel = |p: &Path| p.strip_prefix(root).unwrap_or(p).display().to_string();

        match self.tier {
            NukeTier::Src => eprintln!("nuke: the source, leaving the designs it was written from"),
            NukeTier::Docs => eprintln!("nuke: the designs, and the source written from them"),
        }
        eprintln!();

        for c in &self.crates {
            eprintln!("  {}", c.name);
            for f in &c.files {
                eprintln!("    delete  {}", rel(f));
            }
            eprintln!(
                "    stub    {}{}",
                rel(&c.stub),
                if c.is_proc { "  (proc macro)" } else { "" }
            );
        }

        if !self.docs.is_empty() {
            eprintln!();
            eprintln!("  designs");
            for d in &self.docs {
                eprintln!("    delete  {}", rel(d));
            }
        }

        if !self.escaped.is_empty() {
            eprintln!();
            eprintln!("  refused, and left exactly as they are:");
            for (path, why) in &self.escaped {
                eprintln!("    kept    {}  ({})", path.display(), why.why());
            }
        }

        eprintln!();
        eprintln!(
            "{} file(s) across {} crate(s){}",
            self.file_count(),
            self.crates.len(),
            if self.docs.is_empty() {
                String::new()
            } else {
                format!(" and {} design(s)", self.docs.len())
            }
        );
    }
}

/// What a nuke at this tier would take.
pub(crate) fn plan_nuke(cfg: &Config, tier: NukeTier) -> NukePlan {
    // Every source directory. A nuke that covered one group and reported
    // success would leave the rest of the workspace holding source that every
    // later step assumes is gone.
    let mut crates = Vec::new();
    for path in crate::parse::package_dirs_in(&cfg.src_dirs) {
        let src_dir = path.join("src");
        if !src_dir.exists() {
            continue;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let cargo_toml = path.join("Cargo.toml");
        let is_proc = fs::read_to_string(&cargo_toml)
            .map(|c| c.contains("proc-macro = true"))
            .unwrap_or(false);

        let stub = src_dir.join("lib.rs");
        let mut files = Vec::new();
        collect_rs(&src_dir, &stub, &mut files);
        files.sort();

        crates.push(NukedCrate {
            name,
            files,
            stub,
            is_proc,
        });
    }
    crates.sort_by(|a, b| a.name.cmp(&b.name));

    let mut docs = match tier {
        NukeTier::Src => Vec::new(),
        NukeTier::Docs => {
            crate::document::design_templates(cfg)
                .into_iter()
                .filter(is_design)
                .map(|t| t.path)
                .collect()
        },
    };

    // Nothing outside the repository, whatever the walk or the config produced.
    //
    // The stub goes through this as well as the deletions, and that is the half a
    // first version missed. `fs::write` truncates through a symlink, so a `lib.rs`
    // linked to somewhere else had its target overwritten while the run printed the
    // in-repo path, and on a `src_dirs` pointing outside it printed a paragraph
    // saying it had left that directory alone and then destroyed a file in it.
    // Reproduced both ways before this was written.
    let root = cfg.repo_root.clone();
    let mut escaped: Vec<(PathBuf, Refused)> = Vec::new();

    // A crate whose stub cannot be written safely is dropped whole. Its files are
    // its own, so keeping them would delete a crate's source and leave nothing
    // saying what the crate was.
    crates.retain(|c| {
        let why = if is_symlink(&c.stub) {
            Some(Refused::Link)
        } else if !inside(&root, &c.stub) {
            Some(Refused::Outside)
        } else {
            None
        };
        match why {
            None => true,
            Some(why) => {
                escaped.push((c.stub.clone(), why));
                escaped.extend(c.files.iter().map(|f| (f.clone(), Refused::ItsCrateWent)));
                false
            },
        }
    });
    for c in &mut crates {
        c.files.retain(|p| {
            let ok = inside(&root, p);
            if !ok {
                escaped.push((p.clone(), Refused::Outside));
            }
            ok
        });
    }
    docs.retain(|p| {
        let ok = inside(&root, p);
        if !ok {
            escaped.push((p.clone(), Refused::Outside));
        }
        ok
    });

    NukePlan {
        tier,
        crates,
        docs,
        escaped,
    }
}

/// Whether a template is design, and so something the docs tier takes.
///
/// The render walks every `*.md.tmpl` at the mock root, which is right for it and
/// wrong here: `WORKFLOW.md.tmpl` and `PRINCIPLES.md.tmpl` sit in that directory
/// without being design. Nobody rewrites them from a canon, so a nuke that takes
/// them deletes text the tier below has no way to reproduce.
///
/// What is design is a crate's own `DESIGN.md.tmpl` and deep dives, which carry an
/// owner, plus the workspace's `DESIGN.md.tmpl` at the root, which does not.
fn is_design(t: &crate::document::DesignTemplate) -> bool {
    t.owner.is_some() || t.path.file_name().is_some_and(|n| n == "DESIGN.md.tmpl")
}

/// Whether the path is a symlink, without following it.
///
/// A stub is written with `fs::write`, which truncates whatever the name resolves
/// to. Writing through a link is never what "replace this crate's `lib.rs`" means,
/// even where the target is inside the repository, so the link is refused rather
/// than followed.
fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

/// Every `.rs` file under `dir`, except the one that becomes the stub.
///
/// `file_type()` rather than `Path::is_dir`, and the difference is a deletion outside
/// the repository. `is_dir` follows a symlink, so a link under `src/` pointing anywhere
/// on the machine was descended and its contents removed through it, while the listing
/// printed the in-repo path the link sits at. Reproduced before this was written: a file
/// deleted from a directory git had never seen, on a clean tree, with the plan naming
/// `mock/crates/alpha/src/linked/precious.rs`.
///
/// A symlink is skipped rather than reported. Nothing about the round flow puts one under
/// a source directory, and the tier is source files this repository owns.
fn collect_rs(dir: &Path, keep: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        if kind.is_dir() {
            collect_rs(&path, keep, out);
        } else if path.extension().map(|e| e == "rs").unwrap_or(false) && path != keep {
            out.push(path);
        }
    }
}

/// Whether `path` really sits under `root`, with both sides resolved.
///
/// The walk above closes the symlink route into the plan. This closes the rest: a
/// `src_dirs` entry is `mock_dir.join(d)` with no containment check of its own, so
/// `src_dirs = ["../../outside"]` puts the whole deletion set outside the tree that
/// `tree_is_recoverable` looked at, and the guard passes while nothing it examined is
/// what goes.
///
/// `canonicalize` on the parent rather than on the path, because the path is about to be
/// deleted and a file that is already gone cannot be resolved.
fn inside(root: &Path, path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    match (parent.canonicalize(), root.canonicalize()) {
        (Ok(p), Ok(r)) => p.starts_with(r),
        _ => false,
    }
}

/// Carry out a plan, deleting exactly the files it names.
pub(crate) fn apply(plan: &NukePlan, cfg: &Config) -> ExitCode {
    let mut nuked_files = 0u32;
    // A named file that would not go. Reported and the run fails, because the
    // alternative is a shorter count under a `NUKE complete` line and a success
    // exit, which is the understating direction this shape is built against.
    let mut refused: Vec<(PathBuf, std::io::Error)> = Vec::new();

    for c in &plan.crates {
        for f in &c.files {
            match fs::remove_file(f) {
                Ok(()) => nuked_files += 1,
                Err(e) => refused.push((f.clone(), e)),
            }
        }

        let stub = if c.is_proc {
            format!(
                "//! {}: proc macro crate.\n\
                 //!\n\
                 //! {}. Rewrite from design docs (mechanical, no reinterpretation).\n\
                 \n\
                 extern crate proc_macro;\n",
                c.name, cfg.nuke_marker
            )
        } else {
            format!(
                "//! {}: nuked.\n\
                 //!\n\
                 //! {}. Rewrite from design docs (mechanical, no reinterpretation).\n",
                c.name, cfg.nuke_marker
            )
        };
        if c.stub.exists() {
            nuked_files += 1;
        }
        fs::write(&c.stub, &stub).expect("failed to write lib.rs stub");
        eprintln!("  nuked: {}", c.name);

        // The directories the deleted files sat in, now that they are empty.
        // Left behind, an empty module directory reads as a module somebody
        // forgot to write rather than one that was taken.
        //
        // Only the ones this run emptied. Walking the whole tree also took
        // directories that were already empty and had nothing to do with the
        // nuke, unlisted, in a listing whose point is that it names everything.
        for f in &c.files {
            if let Some(parent) = f.parent() {
                prune_if_empty(parent, c.stub.parent().unwrap_or(&c.stub));
            }
        }
    }

    let mut nuked_docs = 0u32;
    for d in &plan.docs {
        match fs::remove_file(d) {
            Ok(()) => {
                nuked_docs += 1;
                eprintln!("  nuked: {}", d.display());
            },
            Err(e) => refused.push((d.clone(), e)),
        }
    }

    eprintln!();
    eprintln!(
        "--- NUKE complete: {nuked_files} file(s) across {} crate(s), {nuked_docs} design(s) ---",
        plan.crates.len()
    );

    if !refused.is_empty() {
        eprintln!();
        eprintln!("{} named file(s) could not be removed:", refused.len());
        for (path, why) in &refused {
            eprintln!("    {}: {why}", path.display());
        }
        eprintln!("    the counts above are what went, not what the plan named.");
        return ExitCode::FAILURE;
    }

    eprintln!("    cargo check will fail until source is rewritten");
    ExitCode::SUCCESS
}

/// Remove `dir` if the nuke emptied it, then its parent, and so on up to `stop`.
///
/// Bounded by the crate's own `src/`, so a nuke never removes a directory above
/// the tree it was given.
fn prune_if_empty(dir: &Path, stop: &Path) {
    let mut here = dir.to_path_buf();
    while here.starts_with(stop) && here != stop {
        let Ok(mut entries) = fs::read_dir(&here) else {
            return;
        };
        if entries.next().is_some() {
            return;
        }
        if fs::remove_dir(&here).is_err() {
            return;
        }
        let Some(parent) = here.parent() else {
            return;
        };
        here = parent.to_path_buf();
    }
}

/// Ask, once, having said what would go.
///
/// Reads from the terminal rather than from a flag, because the flag is the
/// thing that was easy to reach for by accident and the whole point is that the
/// list is in front of you when you answer. Anything but a plain yes is a no; a
/// closed input, which is what a pipe or a script gives, is also a no.
pub(crate) fn confirm() -> bool {
    use std::io::Write;
    eprint!("go ahead? [y/N] ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) | Err(_) => false,
        Ok(_) => matches!(line.trim(), "y" | "Y" | "yes" | "Yes"),
    }
}

#[cfg(test)]
mod what_makes_a_deletion_recoverable {
    use super::*;

    fn repo(files: &[(&str, &str)], commit: bool) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(d.path())
                .args(args)
                .output()
                .unwrap();
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "t@example.invalid"]);
        run(&["config", "user.name", "t"]);
        for (name, text) in files {
            fs::write(d.path().join(name), text).unwrap();
        }
        if commit {
            run(&["add", "-A"]);
            run(&["commit", "-q", "-m", "one"]);
        }
        d
    }

    #[test]
    fn a_clean_tree_is_recoverable() {
        let d = repo(&[("a.rs", "fn a() {}\n")], true);
        assert!(tree_is_recoverable(d.path()).is_ok());
    }

    #[test]
    fn an_untracked_file_is_the_case_this_exists_for() {
        // The one thing git cannot give back. A source file somebody wrote and
        // has not committed is exactly what a wipe of the source directories
        // would take, and it leaves no object and no reflog entry behind.
        let d = repo(&[("a.rs", "fn a() {}\n")], true);
        fs::write(d.path().join("b.rs"), "fn b() {}\n").unwrap();
        let err = tree_is_recoverable(d.path()).unwrap_err();
        assert!(
            err.contains("b.rs"),
            "the refusal does not name the file: {err}"
        );
    }

    #[test]
    fn a_modification_is_refused_too() {
        let d = repo(&[("a.rs", "fn a() {}\n")], true);
        fs::write(d.path().join("a.rs"), "fn a() { todo!() }\n").unwrap();
        assert!(tree_is_recoverable(d.path()).is_err());
    }

    #[test]
    fn a_directory_that_is_not_a_repository_is_refused() {
        // Not treated as clean. There is nowhere to recover from, which is a
        // stronger reason to refuse than a dirty tree is.
        let d = tempfile::tempdir().unwrap();
        let err = tree_is_recoverable(d.path()).unwrap_err();
        assert!(err.contains("not a git repository"), "{err}");
    }
}

#[cfg(test)]
mod what_a_nuke_takes {
    use super::*;

    /// A mock tree with two crates and every template shape the root and a crate
    /// can carry, which is the smallest tree that tells the two tiers apart and
    /// also tells design apart from the rest of the root.
    fn tree() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let mock = d.path().join("mock");
        fs::create_dir_all(&mock).unwrap();
        fs::write(mock.join("DESIGN.md.tmpl"), "# design\n").unwrap();
        fs::write(mock.join("PRINCIPLES.md.tmpl"), "# principles\n").unwrap();
        fs::write(mock.join("WORKFLOW.md.tmpl"), "# workflow\n").unwrap();

        for name in ["alpha", "beta"] {
            let c = mock.join("crates").join(name);
            fs::create_dir_all(c.join("src").join("inner")).unwrap();
            fs::write(
                c.join("Cargo.toml"),
                format!("[package]\nname = \"{name}\"\n"),
            )
            .unwrap();
            fs::write(c.join("DESIGN.md.tmpl"), "# crate design\n").unwrap();
            fs::write(c.join("DEEPDIVE_layout.md.tmpl"), "# dive\n").unwrap();
            fs::write(c.join("src").join("lib.rs"), "pub mod inner;\n").unwrap();
            fs::write(c.join("src").join("other.rs"), "pub fn other() {}\n").unwrap();
            fs::write(
                c.join("src").join("inner").join("mod.rs"),
                "pub fn i() {}\n",
            )
            .unwrap();
        }
        d
    }

    fn cfg_for(d: &tempfile::TempDir) -> Config {
        Config::from_dir(&d.path().join("mock"))
    }

    #[test]
    fn a_tier_is_named_by_either_of_its_spellings() {
        assert_eq!(NukeTier::parse("src").unwrap(), NukeTier::Src);
        assert_eq!(NukeTier::parse("source").unwrap(), NukeTier::Src);
        assert_eq!(NukeTier::parse("doc").unwrap(), NukeTier::Docs);
        assert_eq!(NukeTier::parse("docs").unwrap(), NukeTier::Docs);
        assert_eq!(NukeTier::parse("design").unwrap(), NukeTier::Docs);
        assert_eq!(NukeTier::parse("designs").unwrap(), NukeTier::Docs);
    }

    #[test]
    fn a_tier_nobody_named_is_refused_and_the_refusal_says_what_there_is() {
        // The control for the test above. A parser that answered `Src` for
        // anything it did not recognise would pass every case there, and would
        // turn a typo into a wipe.
        let err = NukeTier::parse("everything").unwrap_err();
        assert!(err.contains("everything"), "{err}");
        assert!(err.contains("src") && err.contains("docs"), "{err}");
    }

    #[test]
    fn the_source_tier_names_every_module_and_leaves_the_designs_out() {
        let d = tree();
        let plan = plan_nuke(&cfg_for(&d), NukeTier::Src);

        assert_eq!(plan.crates.len(), 2);
        assert!(plan.docs.is_empty(), "the source tier touches no design");

        let alpha = plan.crates.iter().find(|c| c.name == "alpha").unwrap();
        let named: Vec<String> = alpha
            .files
            .iter()
            .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(named.contains(&"other.rs".to_string()), "{named:?}");
        assert!(
            named.contains(&"mod.rs".to_string()),
            "a nested module too: {named:?}"
        );
        assert!(
            !named.contains(&"lib.rs".to_string()),
            "lib.rs is stubbed, not deleted: {named:?}"
        );
        assert_eq!(alpha.stub, alpha.stub.parent().unwrap().join("lib.rs"));
    }

    #[test]
    fn the_design_tier_takes_the_source_with_it() {
        // Not designs alone. A design may only change once the code beneath it
        // is gone, so there is no tier that leaves source standing under a
        // design that was rewritten.
        let d = tree();
        let plan = plan_nuke(&cfg_for(&d), NukeTier::Docs);

        assert_eq!(plan.crates.len(), 2, "the source goes as well");
        let names: Vec<String> = plan
            .docs
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"DESIGN.md.tmpl".to_string()), "{names:?}");
        // Both crate designs, not only the root one, and the deep dives with them.
        assert_eq!(
            names.iter().filter(|n| *n == "DESIGN.md.tmpl").count(),
            3,
            "one at the root and one per crate: {names:?}"
        );
        assert_eq!(
            names
                .iter()
                .filter(|n| *n == "DEEPDIVE_layout.md.tmpl")
                .count(),
            2,
            "a deep dive is design and goes with its crate: {names:?}"
        );
    }

    #[test]
    fn the_design_tier_leaves_the_root_templates_that_are_not_design() {
        // `WORKFLOW` and `PRINCIPLES` sit beside `DESIGN` at the mock root and are
        // not design: nobody rewrites them from a canon, so the tier below cannot
        // reproduce them and a nuke that took them would be a deletion with no
        // recovery. The docs tier used to take every `*.md.tmpl` at that root,
        // which is what the renderer wants and not what this tier means.
        let d = tree();
        let cfg = cfg_for(&d);
        let plan = plan_nuke(&cfg, NukeTier::Docs);

        let names: Vec<String> = plan
            .docs
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(
            !names.contains(&"PRINCIPLES.md.tmpl".to_string()),
            "{names:?}"
        );
        assert!(
            !names.contains(&"WORKFLOW.md.tmpl".to_string()),
            "{names:?}"
        );

        apply(&plan, &cfg);
        assert!(d.path().join("mock/PRINCIPLES.md.tmpl").exists());
        assert!(d.path().join("mock/WORKFLOW.md.tmpl").exists());
        // The control: the root design did go, so the tier still reaches the root.
        assert!(!d.path().join("mock/DESIGN.md.tmpl").exists());
    }

    #[test]
    fn what_the_docs_tier_takes_is_exactly_the_designs() {
        // Set equality rather than two membership checks. Both of the tests above
        // pass on a plan carrying an extra template nobody thought to name, and a
        // template added to the root later is exactly the thing that would slip in.
        let d = tree();
        let cfg = cfg_for(&d);

        let mut got: Vec<PathBuf> = plan_nuke(&cfg, NukeTier::Docs).docs;
        got.sort();

        let mock = d.path().join("mock");
        let mut want = vec![mock.join("DESIGN.md.tmpl")];
        for name in ["alpha", "beta"] {
            let c = mock.join("crates").join(name);
            want.push(c.join("DESIGN.md.tmpl"));
            want.push(c.join("DEEPDIVE_layout.md.tmpl"));
        }
        want.sort();

        assert_eq!(got, want);
    }

    #[test]
    fn apply_deletes_exactly_what_the_plan_named() {
        // The reason the plan is a list rather than a count. A listing built by
        // one walk and a deletion driven by another drift, and they drift in
        // the direction where the listing understates what goes.
        let d = tree();
        let cfg = cfg_for(&d);
        let plan = plan_nuke(&cfg, NukeTier::Docs);

        let planned: Vec<PathBuf> = plan
            .crates
            .iter()
            .flat_map(|c| c.files.iter().cloned())
            .chain(plan.docs.iter().cloned())
            .collect();
        let survivors: Vec<PathBuf> = plan.crates.iter().map(|c| c.stub.clone()).collect();

        apply(&plan, &cfg);

        for p in &planned {
            assert!(!p.exists(), "named and still here: {}", p.display());
        }
        for s in &survivors {
            assert!(s.exists(), "stubbed away entirely: {}", s.display());
            let text = fs::read_to_string(s).unwrap();
            assert!(
                text.contains(&cfg.nuke_marker),
                "the stub carries no marker: {text}"
            );
        }
        // The manifests are not source and are not the design tier.
        assert!(d.path().join("mock/crates/alpha/Cargo.toml").exists());
    }

    #[test]
    fn a_source_nuke_leaves_every_design_on_disk() {
        // The control for the tier split. A `plan_nuke` that ignored its tier
        // and always gathered the designs would pass the two tier tests above,
        // because both of those only check that the right things are present.
        let d = tree();
        let cfg = cfg_for(&d);
        let plan = plan_nuke(&cfg, NukeTier::Src);
        apply(&plan, &cfg);

        assert!(d.path().join("mock/DESIGN.md.tmpl").exists());
        assert!(d.path().join("mock/PRINCIPLES.md.tmpl").exists());
        assert!(d.path().join("mock/crates/alpha/DESIGN.md.tmpl").exists());
    }

    #[test]
    fn the_nuke_takes_a_subset_of_what_the_renderer_writes() {
        // Both sides read `document::design_templates`, so what this constrains is
        // that `document::plan` neither drops a template nor invents one. A
        // template `design_templates` itself misses passes here and is not what
        // this measures; the one enumeration is the thing that makes that the only
        // remaining way for the two to part company.
        //
        // The relation is containment rather than equality, because the renderer
        // writes every root template and the nuke takes only the designs. What the
        // renderer holds and the nuke does not is named below, so a template
        // quietly falling out of the nuke fails here rather than passing as one
        // more member of a difference nobody enumerated.
        let d = tree();
        let cfg = cfg_for(&d);

        let from_nuke: Vec<PathBuf> = plan_nuke(&cfg, NukeTier::Docs).docs;
        let from_renderer: Vec<PathBuf> = crate::document::plan(&cfg, &Default::default())
            .into_iter()
            .filter_map(|p| {
                match p.source {
                    crate::document::Source::Template(path) => Some(path),
                    _ => None,
                }
            })
            .collect();

        assert!(!from_nuke.is_empty(), "an empty containment is not one");
        for p in &from_nuke {
            assert!(
                from_renderer.contains(p),
                "the nuke takes a template the renderer never writes: {}",
                p.display()
            );
        }

        let mut only_rendered: Vec<String> = from_renderer
            .iter()
            .filter(|p| !from_nuke.contains(p))
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        only_rendered.sort();
        assert_eq!(only_rendered, vec![
            "PRINCIPLES.md.tmpl".to_string(),
            "WORKFLOW.md.tmpl".to_string()
        ]);
    }
}

#[cfg(test)]
mod what_the_command_line_asks_for {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_string()).collect()
    }

    #[test]
    fn nothing_is_asked_for_when_the_flag_is_absent() {
        assert!(read_request(&args(&["--check"])).is_none());
        assert!(read_request(&args(&[])).is_none());
    }

    #[test]
    fn a_flag_that_merely_starts_the_same_way_is_not_this_one() {
        // The control for the prefix match. `starts_with("--nuke")` would read
        // a future `--nuke-report` as a nuke and wipe the tree on a flag whose
        // whole purpose was to avoid that.
        assert!(read_request(&args(&["--nuke-nothing"])).is_none());
    }

    #[test]
    fn the_flag_alone_is_a_complete_request() {
        // What this exists for. It used to refuse without `--i-mean-it`, which
        // is the extra detail nobody should have to carry.
        let r = read_request(&args(&["--nuke"])).unwrap().unwrap();
        assert_eq!(r.tier, NukeTier::Src);
        assert!(r.ask, "and it asks, having said what would go");
    }

    #[test]
    fn either_word_answers_the_question_in_advance() {
        for word in ["--y", "--i-mean-it"] {
            let r = read_request(&args(&["--nuke", word])).unwrap().unwrap();
            assert!(!r.ask, "{word} did not answer it");
            assert_eq!(r.tier, NukeTier::Src);
        }
    }

    #[test]
    fn the_tier_rides_on_the_flag() {
        let r = read_request(&args(&["--nuke=docs"])).unwrap().unwrap();
        assert_eq!(r.tier, NukeTier::Docs);
        assert!(r.ask);

        let r = read_request(&args(&["--nuke=docs", "--y"]))
            .unwrap()
            .unwrap();
        assert_eq!(r.tier, NukeTier::Docs);
        assert!(!r.ask);
    }

    #[test]
    fn a_tier_nobody_named_is_a_refusal_rather_than_a_default() {
        // The failure that matters: falling back to `Src` on an unreadable
        // tier turns `--nuke=deisgn` into a source wipe nobody asked for.
        let err = read_request(&args(&["--nuke=everything"]))
            .unwrap()
            .unwrap_err();
        assert!(err.contains("everything"), "{err}");
    }
}

#[cfg(test)]
mod what_the_nuke_will_not_reach {
    use super::*;

    /// A mock tree plus a directory outside it, linked to from inside.
    fn tree_with_a_link_out() -> (tempfile::TempDir, tempfile::TempDir) {
        let d = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let src = d.path().join("mock/crates/alpha/src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            d.path().join("mock/crates/alpha/Cargo.toml"),
            "[package]\nname = \"alpha\"\n",
        )
        .unwrap();
        fs::write(src.join("lib.rs"), "pub mod x;\n").unwrap();
        fs::write(src.join("own.rs"), "pub fn own() {}\n").unwrap();

        fs::write(outside.path().join("precious.rs"), "fn precious() {}\n").unwrap();
        std::os::unix::fs::symlink(outside.path(), src.join("linked")).unwrap();
        (d, outside)
    }

    #[test]
    fn a_symlink_out_of_the_tree_is_not_followed_and_not_deleted_through() {
        // Reproduced against the previous walk before this was written: the file
        // below was deleted from a directory git had never seen, on a clean tree,
        // while the plan printed `mock/crates/alpha/src/linked/precious.rs` as
        // though it were in the repository.
        let (d, outside) = tree_with_a_link_out();
        let cfg = Config::from_dir(&d.path().join("mock"));
        let plan = plan_nuke(&cfg, NukeTier::Src);

        let named: Vec<String> = plan.crates[0]
            .files
            .iter()
            .map(|f| f.display().to_string())
            .collect();
        assert!(
            !named.iter().any(|f| f.contains("precious")),
            "the plan reached through the link: {named:?}"
        );

        apply(&plan, &cfg);
        assert!(
            outside.path().join("precious.rs").exists(),
            "a file outside the repository was deleted"
        );
        // The control: the crate's own module did go, so the walk still works.
        assert!(!d.path().join("mock/crates/alpha/src/own.rs").exists());
    }

    #[test]
    fn a_source_directory_pointed_outside_the_repository_is_refused_and_reported() {
        // `src_dirs` is joined onto the mock dir with no containment check of its
        // own, so this shape puts the whole deletion set outside the tree that
        // `tree_is_recoverable` inspected, and that guard passes.
        let d = tempfile::tempdir().unwrap();
        let mock = d.path().join("repo/mock");
        fs::create_dir_all(&mock).unwrap();
        fs::write(
            mock.join("mockspace.toml"),
            "src_dirs = [\"../../outside\"]\n",
        )
        .unwrap();
        fs::create_dir_all(d.path().join("repo/.git")).unwrap();

        let outside = d.path().join("outside/beta/src");
        fs::create_dir_all(&outside).unwrap();
        fs::write(
            d.path().join("outside/beta/Cargo.toml"),
            "[package]\nname = \"beta\"\n",
        )
        .unwrap();
        fs::write(outside.join("lib.rs"), "pub mod y;\n").unwrap();
        fs::write(outside.join("other.rs"), "pub fn other() {}\n").unwrap();

        let cfg = Config::from_dir(&mock);
        let plan = plan_nuke(&cfg, NukeTier::Src);

        let named: Vec<&PathBuf> = plan.crates.iter().flat_map(|c| c.files.iter()).collect();
        assert!(
            named.is_empty(),
            "the plan kept paths outside the repo: {named:?}"
        );
        assert!(
            !plan.escaped.is_empty(),
            "and it dropped them silently rather than reporting them"
        );

        apply(&plan, &cfg);
        assert!(outside.join("other.rs").exists(), "it deleted them anyway");
    }

    #[test]
    fn the_count_the_plan_prints_is_the_count_apply_reports() {
        // On a crate with no `lib.rs` the plan used to add one for a stub that
        // `apply` would write but not count, so the two disagreed by one and the
        // listing overstated. A listing that disagrees with the deletion is the
        // one thing this shape exists to prevent.
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("mock/crates/alpha/src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            d.path().join("mock/crates/alpha/Cargo.toml"),
            "[package]\nname = \"alpha\"\n",
        )
        .unwrap();
        fs::write(src.join("main.rs"), "fn main() {}\n").unwrap();

        let cfg = Config::from_dir(&d.path().join("mock"));
        let plan = plan_nuke(&cfg, NukeTier::Src);
        assert!(!plan.crates[0].stub.exists(), "the fixture has no lib.rs");
        assert_eq!(
            plan.file_count(),
            plan.crates[0].files.len(),
            "the stub is counted although apply will not count it"
        );

        // The control: where a lib.rs is there, it does count.
        fs::write(&plan.crates[0].stub, "pub mod z;\n").unwrap();
        let plan = plan_nuke(&cfg, NukeTier::Src);
        assert_eq!(plan.file_count(), plan.crates[0].files.len() + 1);
    }

    #[test]
    fn a_directory_that_was_already_empty_is_left_where_it_was() {
        // The prune used to walk the whole source tree and take any empty
        // directory, including ones the nuke had nothing to do with, and it took
        // them unlisted in a listing whose point is that it names everything.
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("mock/crates/alpha/src");
        fs::create_dir_all(src.join("emptied/deeper")).unwrap();
        fs::create_dir_all(src.join("was_empty_before")).unwrap();
        fs::write(
            d.path().join("mock/crates/alpha/Cargo.toml"),
            "[package]\nname = \"alpha\"\n",
        )
        .unwrap();
        fs::write(src.join("lib.rs"), "pub mod emptied;\n").unwrap();
        fs::write(src.join("emptied/deeper/mod.rs"), "pub fn d() {}\n").unwrap();

        let cfg = Config::from_dir(&d.path().join("mock"));
        let plan = plan_nuke(&cfg, NukeTier::Src);
        apply(&plan, &cfg);

        assert!(
            !src.join("emptied").exists(),
            "the directory the nuke emptied is still there"
        );
        assert!(
            src.join("was_empty_before").exists(),
            "a directory the nuke never touched was taken"
        );
    }
}

#[cfg(test)]
mod what_the_nuke_will_not_write {
    use super::*;

    /// The half a first fix missed. `fs::write` truncates through a symlink, and
    /// the stub is the one write in the whole path, so closing the deletions left
    /// the destruction intact and gave it a listing that reassured.
    #[test]
    fn a_stub_never_writes_through_a_symlink_to_somewhere_else() {
        let d = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let src = d.path().join("mock/crates/alpha/src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            d.path().join("mock/crates/alpha/Cargo.toml"),
            "[package]\nname = \"alpha\"\n",
        )
        .unwrap();
        fs::write(src.join("own.rs"), "pub fn own() {}\n").unwrap();

        let precious = outside.path().join("precious_lib.rs");
        fs::write(&precious, "THE ONLY COPY, tracked by nothing\n").unwrap();
        std::os::unix::fs::symlink(&precious, src.join("lib.rs")).unwrap();

        let cfg = Config::from_dir(&d.path().join("mock"));
        let plan = plan_nuke(&cfg, NukeTier::Src);
        apply(&plan, &cfg);

        assert_eq!(
            fs::read_to_string(&precious).unwrap(),
            "THE ONLY COPY, tracked by nothing\n",
            "the stub was written through the link and truncated the target"
        );
        assert!(
            plan.crates.is_empty(),
            "the crate should be dropped whole, not stubbed"
        );
        assert!(
            plan.escaped
                .iter()
                .any(|(p, w)| p.ends_with("lib.rs") && *w == Refused::Link),
            "and the refusal should name the stub: {:?}",
            plan.escaped
        );
        // The control: the crate's own file was not deleted either, because a
        // crate whose stub cannot be written is dropped whole rather than having
        // its source taken with nothing left saying what it was.
        assert!(src.join("own.rs").exists());
    }

    #[test]
    fn a_stub_outside_the_repository_is_refused_along_with_its_crate() {
        // `src_dirs = ["../../outside"]`. The first fix escaped the deletions and
        // then overwrote `lib.rs` in that same directory, printing a paragraph
        // saying it had left the directory alone.
        let d = tempfile::tempdir().unwrap();
        let mock = d.path().join("repo/mock");
        fs::create_dir_all(&mock).unwrap();
        fs::create_dir_all(d.path().join("repo/.git")).unwrap();
        fs::write(
            mock.join("mockspace.toml"),
            "src_dirs = [\"../../outside\"]\n",
        )
        .unwrap();

        let outside = d.path().join("outside/beta/src");
        fs::create_dir_all(&outside).unwrap();
        fs::write(
            d.path().join("outside/beta/Cargo.toml"),
            "[package]\nname = \"beta\"\n",
        )
        .unwrap();
        fs::write(outside.join("lib.rs"), "pub mod y;\n").unwrap();
        fs::write(outside.join("other.rs"), "pub fn other() {}\n").unwrap();

        let cfg = Config::from_dir(&mock);
        let plan = plan_nuke(&cfg, NukeTier::Src);
        assert!(plan.all_escaped(), "the plan should say everything escaped");
        apply(&plan, &cfg);

        assert_eq!(
            fs::read_to_string(outside.join("lib.rs")).unwrap(),
            "pub mod y;\n",
            "a file outside the repository was overwritten with the stub"
        );
        assert!(outside.join("other.rs").exists());
    }

    #[test]
    fn a_symlink_inside_the_repository_is_still_not_followed() {
        // This is what isolates the walk from the containment check. With a
        // target inside the tree, `inside` returns true and the `is_symlink` skip
        // in `collect_rs` is the only thing standing; restoring the old
        // `path.is_dir()` walk deletes through the link and every other test in
        // this file stays green.
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("mock/crates/alpha/src");
        let elsewhere = d.path().join("mock/research/probes");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&elsewhere).unwrap();
        fs::write(
            d.path().join("mock/crates/alpha/Cargo.toml"),
            "[package]\nname = \"alpha\"\n",
        )
        .unwrap();
        fs::write(src.join("lib.rs"), "pub mod x;\n").unwrap();
        fs::write(src.join("own.rs"), "pub fn own() {}\n").unwrap();
        fs::write(elsewhere.join("probe.rs"), "fn probe() {}\n").unwrap();
        std::os::unix::fs::symlink(&elsewhere, src.join("linked")).unwrap();

        let cfg = Config::from_dir(&d.path().join("mock"));
        let plan = plan_nuke(&cfg, NukeTier::Src);
        apply(&plan, &cfg);

        assert!(
            elsewhere.join("probe.rs").exists(),
            "the walk followed a link and deleted a file outside the source tree"
        );
        // The control: the crate's own module still went, so the walk works.
        assert!(!src.join("own.rs").exists());
    }
}
