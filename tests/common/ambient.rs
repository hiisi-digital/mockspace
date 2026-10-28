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
