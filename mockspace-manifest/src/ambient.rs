//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a test must take out of a child's environment so the machine it runs on
//! cannot answer for the repository or the identity under test.
//!
//! The lists are here, in the one package that every test target of the
//! repository can reach and that ships whole, so a package that publishes its
//! tests scrubs from inside itself rather than through a path out of the
//! package root. The repository's integration and lib tests reach it through
//! `tests/common/ambient.rs`, which re-exports this module and expands
//! [`scrub_before_main!`]. It names nothing but `std`.

use std::process::Command;

/// The variables a parent git leaves in the environment of anything it runs, a
/// hook above all, which select the repository. A suite run from inside a hook
/// would otherwise make every child git (and every generated hook) act on the
/// parent's repository instead of the fixture's.
pub const REPOSITORY_VARIABLES: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_PREFIX",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
];

/// The session's own identity. The workspace supplies one through `GIT_AUTHOR_*`
/// and `GIT_COMMITTER_*`, and an ambient override beats the repository's
/// `user.name` a test sets, so a test about which identity reaches a gate would
/// be measuring the machine's instead. The last two carry configuration through
/// the environment, which can override a repository's own.
pub const IDENTITY_VARIABLES: &[&str] = &[
    "GIT_AUTHOR_NAME",
    "GIT_AUTHOR_EMAIL",
    "GIT_AUTHOR_DATE",
    "GIT_COMMITTER_NAME",
    "GIT_COMMITTER_EMAIL",
    "GIT_COMMITTER_DATE",
    "EMAIL",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
];

/// Take the machine's identity out of a child's environment. Applied before the
/// test's own variables, which are the ones meant to count.
pub fn without_ambient_identity(c: &mut Command) -> &mut Command {
    for k in IDENTITY_VARIABLES {
        c.env_remove(k);
    }
    c
}

/// Take the variables that select a repository out of a child's environment.
pub fn without_ambient_repo(c: &mut Command) -> &mut Command {
    for k in REPOSITORY_VARIABLES {
        c.env_remove(k);
    }
    c
}

/// Both scrubs, for a child that is a git or a hook.
pub fn clean_of_the_machine(c: &mut Command) -> &mut Command {
    without_ambient_identity(c);
    without_ambient_repo(c)
}

/// The variables that carry git configuration in the environment: the count form
/// and its numbered keys and values, and the older single string.
fn is_config_variable(k: &str) -> bool {
    k == "GIT_CONFIG_COUNT"
        || k == "GIT_CONFIG_PARAMETERS"
        || k.starts_with("GIT_CONFIG_KEY_")
        || k.starts_with("GIT_CONFIG_VALUE_")
}

/// Drop what the machine says about git and say what a test may rely on: no
/// repository selected, no configuration from the environment, no global or system
/// configuration file, and one fixed identity that a test may override per child.
///
/// A suite started inside a repository (a hook, or `GIT_DIR=... cargo test`) has
/// those variables in its own environment, and a child's working directory does
/// not outrank them. The per-command scrubs above reach a child a test spawns;
/// this reaches the ones they cannot: engine code the test calls in process, and
/// helpers that spawn git without asking.
///
/// # Safety
///
/// The environment is process-global and unsynchronised. Call it before any other
/// thread exists, which is what [`scrub_before_main!`] does.
pub unsafe fn scrub_process() {
    let config: Vec<String> = std::env::vars_os()
        .filter_map(|(k, _)| k.into_string().ok())
        .filter(|k| is_config_variable(k))
        .collect();
    for k in REPOSITORY_VARIABLES
        .iter()
        .chain(IDENTITY_VARIABLES)
        .copied()
        .map(String::from)
        .chain(config)
    {
        // SAFETY: the caller guarantees no other thread exists yet.
        unsafe { std::env::remove_var(k) };
    }
    for (k, v) in [
        ("GIT_CONFIG_GLOBAL", "/dev/null"),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_AUTHOR_NAME", "mockspace test"),
        ("GIT_AUTHOR_EMAIL", "test@example.invalid"),
        ("GIT_COMMITTER_NAME", "mockspace test"),
        ("GIT_COMMITTER_EMAIL", "test@example.invalid"),
    ] {
        // SAFETY: as above.
        unsafe { std::env::set_var(k, v) };
    }
}

/// Runs [`scrub_process`] before `main`, while the process is one thread, so that
/// expanding this once in a test binary is all it takes to be scrubbed. Linux and
/// macOS; elsewhere the static is inert and the per-command scrubs are what is left.
///
/// Expands to a static in the calling crate, so a second expansion in the same
/// module is a duplicate definition.
#[macro_export]
macro_rules! scrub_before_main {
    () => {
        #[used]
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
        #[cfg_attr(
            any(target_os = "linux", target_os = "android"),
            unsafe(link_section = ".init_array")
        )]
        #[cfg_attr(target_os = "macos", unsafe(link_section = "__DATA,__mod_init_func"))]
        static SCRUB_BEFORE_MAIN: extern "C" fn() = {
            extern "C" fn scrub() {
                // SAFETY: a constructor runs before `main`, on the only thread.
                unsafe { $crate::ambient::scrub_process() };
            }
            scrub
        };
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn removed(c: &Command, k: &str) -> bool {
        c.get_envs().any(|(name, v)| name == k && v.is_none())
    }

    /// The scrub is expanded in this package and not reached through a path out
    /// of it, so a package built from its tarball runs it too. The test binary is
    /// started with `GIT_CONFIG_GLOBAL` pointing at a file; only the scrub that
    /// runs before `main` can have replaced it.
    #[test]
    fn this_test_binary_ran_the_scrub_before_main() {
        assert_eq!(
            std::env::var("GIT_CONFIG_GLOBAL").as_deref(),
            Ok("/dev/null")
        );
        assert_eq!(std::env::var("GIT_CONFIG_NOSYSTEM").as_deref(), Ok("1"));
    }

    #[test]
    fn the_repository_scrub_removes_every_listed_variable_and_only_those() {
        let mut c = Command::new("git");
        without_ambient_repo(&mut c);
        for k in REPOSITORY_VARIABLES {
            assert!(removed(&c, k), "{k} is still inherited");
        }
        assert!(
            !removed(&c, "GIT_AUTHOR_NAME"),
            "the identity is the other scrub's"
        );
        assert_eq!(c.get_envs().count(), REPOSITORY_VARIABLES.len());
    }

    #[test]
    fn the_identity_scrub_removes_every_listed_variable_and_only_those() {
        let mut c = Command::new("git");
        without_ambient_identity(&mut c);
        for k in IDENTITY_VARIABLES {
            assert!(removed(&c, k), "{k} is still inherited");
        }
        assert!(
            !removed(&c, "GIT_DIR"),
            "the repository is the other scrub's"
        );
        assert_eq!(c.get_envs().count(), IDENTITY_VARIABLES.len());
    }

    #[test]
    fn a_configuration_variable_is_recognised_in_each_of_its_forms() {
        for k in [
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_PARAMETERS",
            "GIT_CONFIG_KEY_0",
            "GIT_CONFIG_VALUE_12",
        ] {
            assert!(is_config_variable(k), "{k}");
        }
        for k in ["GIT_CONFIG_GLOBAL", "GIT_CONFIG_NOSYSTEM", "GIT_DIR", "CONFIG_COUNT"] {
            assert!(!is_config_variable(k), "{k}");
        }
    }
}
