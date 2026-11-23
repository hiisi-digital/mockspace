//! Keeps git from writing over, or losing, the files of an instance.
//!
//! The files of an instance are hidden from the branch by `.git/info/exclude`, and git treats an
//! ignored file as expendable: a checkout of a commit that tracks the same paths (a branch cut
//! before the migration) overwrites them without a word, and leaving that commit removes them.
//! There is no `pre-checkout` hook, the hooks that do run (`post-checkout`) run after the working
//! tree was written, and no configuration makes git refuse to overwrite an ignored file; only the
//! `--no-overwrite-ignore` flag does, and nothing can put a flag on a person's command.
//!
//! Two things are done against that. A sparse checkout: a path the patterns leave out is not
//! written, and one that is there and differs from the commit is left in place with a warning,
//! never replaced. So each worktree carries a managed block of patterns, one set for every declared
//! ref, written from the same lines as the exclude file. The switch that turns it on sits in the
//! common config, so a bare clone's `core.bare` stays where its linked worktrees read it, and each
//! worktree keeps its own patterns file; a worktree with none reads every path, and one an earlier
//! sparse checkout left is made to read every path before the switch reaches it.
//! `sparse.expectFilesOutsideOfPatterns` keeps an edited instance file skipped at a commit from
//! before the migration, so the forced ways back leave it on disk. And the hooks of `hook`: a merge
//! that conflicts on a path the migration took off the branch writes the branch's bytes whatever
//! the patterns say, but git moves `ORIG_HEAD` first, and a checkout moves `HEAD`, and the hooks
//! copy the files of every instance into its backup ref, so the next engine command names what git
//! wrote over and `restore` brings it back; the monitor of `monitor` copies them before git reads the
//! work tree while the index tracks any of them. `instantiate`, `commit` and the migration write both;
//! `verify` checks both and names a block whose declaration is gone. Every file this changes is
//! kept in the person's cache first (`mockspace_store::kept`).

use std::path::{Path, PathBuf};

use mockspace_store::{Git, Held, sparse_lines};

use crate::config::Config;

const BEGIN: &str = "# mockspace data refs: the instances git must not write over (managed, do not edit)";
const END: &str = "# end mockspace data refs";

fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    Git::new(repo.to_path_buf()).must(args, &[], None).map(|o| o.text()).map_err(|e| e.to_string())
}

/// A config value, `None` where it is not set; a failed read is an error.
fn config(repo: &Path, args: &[&str], key: &str) -> Result<Option<String>, String> {
    Git::new(repo.to_path_buf()).config_get(args, key).map_err(|e| e.to_string())
}

/// The path of `dir` from the repository root, `/`-separated, with no slash at either end.
fn from_root(cfg: &Config, dir: &Path) -> String {
    dir.strip_prefix(&cfg.repo_root)
        .map(|rel| rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
        .unwrap_or_default()
}

/// The sparse-checkout lines of every declared ref, anchored at the repository root, from the
/// same writer as the exclude lines of the instance.
pub fn managed_lines(cfg: &Config) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for d in &cfg.data_refs {
        let rel = from_root(cfg, &d.instance_dir(&cfg.mock_dir));
        let held = match d.include.as_deref() {
            None => None,
            Some(patterns) => Some(Held::new(patterns).map_err(|e| e.to_string())?),
        };
        out.extend(sparse_lines(&rel, held.as_ref()));
    }
    Ok(out)
}

/// What the sparse-checkout file of this worktree holds: whatever was in it outside the managed
/// block (or `/*`, everything, where it was empty), then the block.
fn compose(existing: &str, lines: &[String]) -> String {
    let mut keep: Vec<&str> = Vec::new();
    let mut inside = false;
    for l in existing.lines() {
        if l == BEGIN {
            inside = true;
        } else if l == END {
            inside = false;
        } else if !inside && !l.trim().is_empty() {
            keep.push(l);
        }
    }
    let mut text = String::new();
    if keep.is_empty() {
        text.push_str("/*\n");
    }
    keep.iter().for_each(|l| text.push_str(&format!("{l}\n")));
    if !lines.is_empty() {
        text.push_str(BEGIN);
        text.push('\n');
        lines.iter().for_each(|l| text.push_str(&format!("{l}\n")));
        text.push_str(END);
        text.push('\n');
    }
    text
}

fn path_of(cfg: &Config, what: &str) -> Result<PathBuf, String> {
    Ok(PathBuf::from(git(&cfg.repo_root, &["rev-parse", "--path-format=absolute", "--git-path", what])?))
}

fn has_block(text: &str) -> bool {
    text.lines().any(|l| l == BEGIN)
}

/// Writes the managed block for this worktree, turns the sparse checkout on for it without applying
/// it (nothing already in the tree is removed by this), and installs the hook that copies an edit.
/// Where no ref is declared any more, takes the block out.
pub fn install(cfg: &Config) -> Result<(), String> {
    let lines = managed_lines(cfg)?;
    let file = path_of(cfg, "info/sparse-checkout")?;
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    if lines.is_empty() {
        if has_block(&existing) {
            keep(&file)?;
            std::fs::write(&file, compose(&existing, &[])).map_err(|e| format!("writing {}: {e}", file.display()))?;
        }
        let _ = std::fs::remove_file(remembered_file(cfg)?);
        return Ok(());
    }
    let repo = &cfg.repo_root;
    // a failed read is an error, never read as the setting being off: the patterns a person keeps
    // would be dropped as left over
    let setting = |key: &str| -> Result<bool, String> { Ok(config(repo, &["--type=bool"], key)?.as_deref() == Some("true")) };
    // lines a sparse checkout that is off left in the file are no patterns of anybody's now, and
    // turning it on would hide the branch's files they leave out
    let was_on = setting("core.sparseCheckout")?;
    let cone = was_on && setting("core.sparseCheckoutCone")?;
    let existing = if was_on { existing } else { String::new() };
    // per-worktree config is never turned on: with it, a bare clone's `core.bare` has to move into
    // the bare repository's own config, and git 2.43 then reads the bare repository from a linked
    // worktree as one with its HEAD branch checked out, so that branch could not be checked out,
    // fetched into or moved there again. The switch goes into the common config, and each worktree
    // keeps its own patterns file; where per-worktree config is on already, it stays the worktree's.
    let per_worktree = setting("extensions.worktreeConfig")?;
    if !was_on && !per_worktree {
        // turning the shared switch on reads the patterns file of every other worktree, and one an
        // earlier sparse checkout left behind would hide the branch's files it leaves out there
        neutral_files_elsewhere(cfg, &file)?;
    }
    let scope: &[&str] = if per_worktree { &["--worktree"] } else { &[] };
    // the edit is kept on disk where git un-skips a file present outside the patterns: at a commit
    // from before the migration the file then stays skipped, and the forced ways back keep it
    let want = [("core.sparseCheckout", "true"), ("core.sparseCheckoutCone", "false"), ("sparse.expectFilesOutsideOfPatterns", "true")];
    let mut kept = false;
    for (key, value) in want {
        if config(repo, &["--type=bool"], key)?.as_deref() == Some(value) {
            continue;
        }
        if !kept {
            keep_config(cfg, per_worktree)?;
            kept = true;
        }
        let mut args = vec!["config"];
        args.extend_from_slice(scope);
        args.extend([key, value]);
        git(repo, &args)?;
    }
    super::monitor::install(cfg, scope, || keep_config(cfg, per_worktree))?;
    if cone {
        eprintln!(
            "data-refs: the sparse checkout of this worktree was in cone mode, and is now in the full pattern mode the \
             instances need; its patterns in {} are read as full patterns from here on, which `git sparse-checkout list` \
             shows",
            file.display()
        );
    }
    let text = compose(&existing, &lines);
    if text != existing {
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        keep(&file)?;
        std::fs::write(&file, text).map_err(|e| format!("writing {}: {e}", file.display()))?;
    }
    let held = remembered_file(cfg)?;
    let declared = serde_json::to_string(&cfg.data_refs).map_err(|e| e.to_string())?;
    if std::fs::read_to_string(&held).ok().as_deref() != Some(declared.as_str()) {
        std::fs::write(&held, declared).map_err(|e| format!("writing {}: {e}", held.display()))?;
    }
    super::hook::install(cfg)
}

/// Where this worktree keeps the declarations it last installed.
fn remembered_file(cfg: &Config) -> Result<PathBuf, String> {
    path_of(cfg, "mock-data-refs.json")
}

/// The declarations this worktree last installed, for the hook that runs on a branch that declares
/// none of them. Nothing where there are none or the file cannot be read.
pub fn remembered(cfg: &Config) -> Vec<super::DataRefDecl> {
    remembered_file(cfg)
        .ok()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Whether this worktree is protected: the sparse checkout is on for it, its file holds the block as
/// it is now declared, and the hook that copies an edit is there. Says what to run where it is not.
pub fn check(cfg: &Config) -> Result<(), String> {
    let lines = managed_lines(cfg)?;
    let file = path_of(cfg, "info/sparse-checkout")?;
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    if lines.is_empty() {
        return if has_block(&existing) {
            Err("this worktree still keeps git from writing over the instance of a ref that is no longer declared (its \
                 paths are hidden from git); `cargo mock data-refs instantiate` takes the block out"
                .to_string())
        } else {
            Ok(())
        };
    }
    let set = |key: &str| -> Result<bool, String> { Ok(config(&cfg.repo_root, &["--type=bool"], key)?.as_deref() == Some("true")) };
    let on = set("core.sparseCheckout")? && !set("core.sparseCheckoutCone")?;
    if !(on && compose(&existing, &lines) == existing) {
        return Err("this worktree does not keep git from writing over its instances (a checkout of a commit from before \
                    the migration would overwrite them); `cargo mock data-refs instantiate` sets that up"
            .to_string());
    }
    // without it git un-skips an edited file at a commit from before the migration, and a forced
    // way back then writes over it
    if !set("sparse.expectFilesOutsideOfPatterns")? {
        return Err("this worktree lets git take an edited instance file for the branch's at a commit from before the \
                    migration (`sparse.expectFilesOutsideOfPatterns` is not set), where a forced way back writes over it; \
                    `cargo mock data-refs instantiate` sets that up"
            .to_string());
    }
    // the hook reads these on a branch that declares none of the refs; a ref missing from them is
    // not copied before git writes over it there
    if remembered(cfg) != cfg.data_refs {
        return Err("this worktree does not remember every declared ref for the hook that copies an edit (a checkout of a \
                    commit from before the migration would overwrite the instances it leaves out); `cargo mock data-refs \
                    instantiate` sets that up"
            .to_string());
    }
    super::hook::check(cfg)?;
    super::monitor::check(cfg)
}

/// Keeps a copy of what stands at `path` in the person's cache before it is written over, changed
/// or removed (`mockspace_store::kept`); a copy that cannot be made stops the write.
fn keep(path: &Path) -> Result<(), String> {
    mockspace_store::kept::keep(path)
        .map(|_| ())
        .map_err(|e| format!("keeping a copy of {} before writing over it: {e}", path.display()))
}

/// Keeps the config files a setting is about to change: the common one, and this worktree's own
/// where per-worktree config is on.
fn keep_config(cfg: &Config, per_worktree: bool) -> Result<(), String> {
    let common = PathBuf::from(git(&cfg.repo_root, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?);
    keep(&common.join("config"))?;
    if per_worktree {
        keep(&path_of(cfg, "config.worktree")?)?;
    }
    Ok(())
}

/// Makes the patterns file of every worktree but this one read every path, before the shared
/// switch turns the sparse checkout on for all of them: lines a sparse checkout that is off left
/// there would otherwise hide the branch's files they leave out at that worktree's next checkout.
/// Each file is kept in the cache first. A worktree with no such file reads every path already.
fn neutral_files_elsewhere(cfg: &Config, own: &Path) -> Result<(), String> {
    let common = PathBuf::from(git(&cfg.repo_root, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?);
    let mut files = vec![common.join("info").join("sparse-checkout")];
    match std::fs::read_dir(common.join("worktrees")) {
        Ok(dirs) => files.extend(dirs.flatten().map(|d| d.path().join("info").join("sparse-checkout"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
        Err(e) => return Err(format!("reading the worktrees of {}: {e}", common.display())),
    }
    let own = own.canonicalize().unwrap_or_else(|_| own.to_path_buf());
    for f in files {
        if f.canonicalize().unwrap_or_else(|_| f.clone()) == own {
            continue;
        }
        match std::fs::read_to_string(&f) {
            Ok(text) if text == "/*\n" => {},
            Ok(_) => {
                keep(&f)?;
                std::fs::write(&f, "/*\n").map_err(|e| format!("writing {}: {e}", f.display()))?;
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
            Err(e) => return Err(format!("reading {}: {e}", f.display())),
        }
    }
    Ok(())
}
