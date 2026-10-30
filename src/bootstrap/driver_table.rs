//! Every merge driver mockspace registers, in one place, so the clone's config and the user's are given
//! the same five.

use super::*;

/// The launcher the engine drivers run through.
const LAUNCHER: &str = "cargo mock";

/// Which git config a driver is written to: one clone's, or the user's, so a clone made later, or in
/// a container that never ran `cargo mock` there, has the drivers git is told to call. `User(Some(file))`
/// is the user's config at that path, for a test; `User(None)` is `git config --global`.
pub(crate) enum ConfigTarget<'a> {
    Clone(&'a Path),
    User(Option<&'a Path>),
}

impl ConfigTarget<'_> {
    /// `git config` over this target with `args`, `None` when git could not be run.
    pub(crate) fn git_config(&self, args: &[&str]) -> Option<std::process::Output> {
        let mut c = Command::new("git");
        match self {
            Self::Clone(repo) => c.arg("-C").arg(repo).args(["config", "--local"]),
            Self::User(None) => c.args(["config", "--global"]),
            Self::User(Some(file)) => c.args(["config", "--file"]).arg(file),
        };
        c.args(args).output().ok()
    }
}

/// Register all five drivers in `target`. Idempotent.
pub(crate) fn register_all_drivers(target: &ConfigTarget, actions: &mut Vec<String>) {
    register_driver(target, actions);
    register_registry_driver(target, LAUNCHER, actions);
    register_round_driver(target, LAUNCHER, actions);
    register_history_driver(target, LAUNCHER, actions);
    register_append_driver(target, LAUNCHER, actions);
}

/// A clone that registered the generated-file driver before its command left the path behind still carries
/// that entry, naming one clone's git directory, and git prefers a clone's entry to the user's. Unset it so
/// the user's takes over. An entry that names no `merge-generated` script is somebody's own and stays.
pub(crate) fn retire_stale_clone_entries(repo_root: &Path, actions: &mut Vec<String>) {
    let target = ConfigTarget::Clone(repo_root);
    let key = format!("merge.{DRIVER}.driver");
    let Some(out) = target.git_config(&[&key]).filter(|o| o.status.success()) else {
        return;
    };
    let current = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if current == generated_driver_command() || !current.contains("mockspace/merge-generated") {
        return;
    }
    if target.git_config(&["--unset", &key]).is_some_and(|o| o.status.success()) {
        actions.push(format!(
            "retired the {DRIVER} entry of this clone, which named a path: {current}"
        ));
    } else {
        actions.push(format!("could not retire the {DRIVER} entry of this clone"));
    }
}

/// `mock register-drivers --global`: register the drivers in the user's git config, so a clone that has
/// not run `cargo mock` yet, or never will, still has them when git is told to call one. Writes to the
/// user's config and nowhere else, which is why it needs the flag spelled out. Run inside a repository it
/// also retires that clone's own entry where the entry names a path (see [`retire_stale_clone_entries`]).
pub fn run_register_drivers(args: &[&str]) -> std::process::ExitCode {
    if args != ["--global"] {
        eprintln!("usage: mock register-drivers --global");
        return std::process::ExitCode::from(2);
    }
    let mut actions = Vec::new();
    register_all_drivers(&ConfigTarget::User(None), &mut actions);
    let top = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()));
    if let Some(top) = top {
        retire_stale_clone_entries(&top, &mut actions);
    }
    if actions.iter().any(|a| a.starts_with("could not")) {
        for a in &actions {
            eprintln!("mock: {a}");
        }
        return std::process::ExitCode::FAILURE;
    }
    for a in &actions {
        println!("{a}");
    }
    std::process::ExitCode::SUCCESS
}
