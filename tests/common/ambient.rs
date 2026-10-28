//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a test must take out of a child's environment so the machine it runs on
//! cannot answer for the repository or the identity under test.
//!
//! One file, included by path from the lib tests (`hooks_identity_tests`) and from
//! the integration tests. It is not a `mod` of `common/mod.rs` because the lib
//! crate cannot reach an integration target's modules, and `common/mod.rs` reads
//! `CARGO_BIN_EXE_mockspace`, which only an integration target has. It names
//! nothing but `std`, which is what lets both include it.

#![allow(dead_code)]

use std::process::Command;

/// The session's own identity. The workspace supplies one through `GIT_AUTHOR_*`
/// and `GIT_COMMITTER_*`, and an ambient override beats the repository's
/// `user.name` the test sets, so a test about which identity reaches a gate
/// would be measuring the machine's instead. Applied before the test's own
/// variables, which are the ones meant to count.
pub fn without_ambient_identity(c: &mut Command) -> &mut Command {
    for k in [
        "GIT_AUTHOR_NAME",
        "GIT_AUTHOR_EMAIL",
        "GIT_AUTHOR_DATE",
        "GIT_COMMITTER_NAME",
        "GIT_COMMITTER_EMAIL",
        "GIT_COMMITTER_DATE",
        "EMAIL",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
    ] {
        c.env_remove(k);
    }
    c
}

/// The variables a parent git leaves in the environment of anything it runs, a
/// hook above all, which select the repository. A suite run from inside a hook
/// would otherwise make every child git (and every generated hook) act on the
/// parent's repository instead of the fixture's.
pub fn without_ambient_repo(c: &mut Command) -> &mut Command {
    for k in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_PREFIX",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
    ] {
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
/// thread exists, which is what [`SCRUB_BEFORE_MAIN`] does.
pub unsafe fn scrub_process() {
    let config: Vec<String> = std::env::vars_os()
        .filter_map(|(k, _)| k.into_string().ok())
        .filter(|k| is_config_variable(k))
        .collect();
    for k in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_PREFIX",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
        "GIT_AUTHOR_DATE",
        "GIT_COMMITTER_DATE",
        "EMAIL",
    ]
    .iter()
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
/// including this file in a test binary is all it takes to be scrubbed. Linux and
/// macOS; elsewhere the static is inert and the per-command scrubs are what is left.
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
        unsafe { scrub_process() };
    }
    scrub
};
