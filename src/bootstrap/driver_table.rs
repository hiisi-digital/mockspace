//! Every merge driver mockspace registers, in one place, so the clone's config and the user's are given
//! the same five.

use super::*;

/// The launcher the engine drivers run through.
const LAUNCHER: &str = "cargo mock";

/// Register all five drivers in `target`. Idempotent.
pub(crate) fn register_all_drivers(target: &ConfigTarget, actions: &mut Vec<String>) {
    register_driver(target, actions);
    register_registry_driver(target, LAUNCHER, actions);
    register_round_driver(target, LAUNCHER, actions);
    register_history_driver(target, LAUNCHER, actions);
    register_append_driver(target, LAUNCHER, actions);
}

/// `mock register-drivers --global`: register the drivers in the user's git config, so a clone that has
/// not run `cargo mock` yet, or never will, still has them when git is told to call one. Writes to the
/// user's config and nowhere else, which is why it needs the flag spelled out.
pub fn run_register_drivers(args: &[&str]) -> std::process::ExitCode {
    if args != ["--global"] {
        eprintln!("usage: mock register-drivers --global");
        return std::process::ExitCode::from(2);
    }
    let mut actions = Vec::new();
    register_all_drivers(&ConfigTarget::User(None), &mut actions);
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
