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
    pub(crate) tier:   NukeTier,
    pub(crate) crates: Vec<NukedCrate>,
    /// Design templates, empty at [`NukeTier::Src`].
    pub(crate) docs:   Vec<PathBuf>,
}

impl NukePlan {
    pub(crate) fn is_empty(&self) -> bool {
        self.crates.is_empty() && self.docs.is_empty()
    }

    pub(crate) fn file_count(&self) -> usize {
        self.crates.iter().map(|c| c.files.len() + 1).sum::<usize>() + self.docs.len()
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

    let docs = match tier {
        NukeTier::Src => Vec::new(),
        NukeTier::Docs => {
            crate::document::design_templates(cfg)
                .into_iter()
                .map(|t| t.path)
                .collect()
        },
    };

    NukePlan {
        tier,
        crates,
        docs,
    }
}

/// Every `.rs` file under `dir`, except the one that becomes the stub.
fn collect_rs(dir: &Path, keep: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, keep, out);
        } else if path.extension().map(|e| e == "rs").unwrap_or(false) && path != keep {
            out.push(path);
        }
    }
}

/// Carry out a plan, deleting exactly the files it names.
pub(crate) fn apply(plan: &NukePlan, cfg: &Config) -> ExitCode {
    let mut nuked_files = 0u32;

    for c in &plan.crates {
        for f in &c.files {
            if fs::remove_file(f).is_ok() {
                nuked_files += 1;
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
        prune_empty_dirs(c.stub.parent().unwrap_or(&c.stub));
    }

    let mut nuked_docs = 0u32;
    for d in &plan.docs {
        if fs::remove_file(d).is_ok() {
            nuked_docs += 1;
            eprintln!("  nuked: {}", d.display());
        }
    }

    eprintln!();
    eprintln!(
        "--- NUKE complete: {nuked_files} file(s) across {} crate(s), {nuked_docs} design(s) ---",
        plan.crates.len()
    );
    eprintln!("    cargo check will fail until source is rewritten");
    ExitCode::SUCCESS
}

fn prune_empty_dirs(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            prune_empty_dirs(&path);
            let _ = fs::remove_dir(&path);
        }
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

    /// A mock tree with two crates and two design templates, the smallest shape
    /// that tells the two tiers apart.
    fn tree() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let mock = d.path().join("mock");
        fs::create_dir_all(&mock).unwrap();
        fs::write(mock.join("DESIGN.md.tmpl"), "# design\n").unwrap();
        fs::write(mock.join("PRINCIPLES.md.tmpl"), "# principles\n").unwrap();

        for name in ["alpha", "beta"] {
            let c = mock.join("crates").join(name);
            fs::create_dir_all(c.join("src").join("inner")).unwrap();
            fs::write(
                c.join("Cargo.toml"),
                format!("[package]\nname = \"{name}\"\n"),
            )
            .unwrap();
            fs::write(c.join("DESIGN.md.tmpl"), "# crate design\n").unwrap();
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
        assert!(
            names.contains(&"PRINCIPLES.md.tmpl".to_string()),
            "{names:?}"
        );
        // Both crate designs, not only the root ones.
        assert_eq!(
            names.iter().filter(|n| *n == "DESIGN.md.tmpl").count(),
            3,
            "one at the root and one per crate: {names:?}"
        );
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
    fn the_nuke_and_the_renderer_agree_on_what_a_design_is() {
        // The one way this can go quietly wrong: a template the renderer knows
        // about and the nuke does not survives a design nuke, and the tier is
        // reported as taken while part of it is still standing. Both read the
        // same enumeration, and this is what says so.
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

        assert_eq!(from_nuke, from_renderer);
        assert!(!from_nuke.is_empty(), "an empty agreement is not one");
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
