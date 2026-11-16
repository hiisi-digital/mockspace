//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

fn lock(notko_rev: &str, libc: &str) -> String {
    format!(
        "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"0.0.0\"\n\n\
         [[package]]\nname = \"libc\"\nversion = \"{libc}\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n\n\
         [[package]]\nname = \"mockspace-bench-core\"\nversion = \"0.0.0\"\nsource = \"git+https://github.com/hiisi-digital/mockspace?branch=dev#aaaa\"\n\n\
         [[package]]\nname = \"notko\"\nversion = \"0.1.0\"\nsource = \"git+https://github.com/orgrinrt/notko.git?branch=dev#{notko_rev}\"\n"
    )
}

/// What a lockfile resolved is every non-path package by name, version and
/// source, the harness's own crates left out.
#[test]
fn resolved_lists_what_came_from_elsewhere_and_not_the_instrument() {
    let r = resolved(&lock("1111", "0.2.170"));
    assert_eq!(
        r,
        "libc 0.2.170 registry+https://github.com/rust-lang/crates.io-index\n\
         notko 0.1.0 git+https://github.com/orgrinrt/notko.git?branch=dev#1111\n"
    );
}

/// The same files and the same resolution are the same source, run after run;
/// a git branch that moved, a newer registry release, or a changed file is a
/// new one; and the instrument moving is not.
#[test]
fn a_source_moves_with_the_code_and_the_resolution_and_nothing_else() {
    let files = "aaaa  live/arms/a/src/lib.rs\nbbbb  ../crates/engine/src/lib.rs\n";
    let base = arm_source(files, Some(&lock("1111", "0.2.170")));
    assert_eq!(base.len(), 12);
    assert_eq!(
        arm_source(files, Some(&lock("1111", "0.2.170"))),
        base,
        "unchanged: the same point"
    );
    assert_ne!(
        arm_source(files, Some(&lock("2222", "0.2.170"))),
        base,
        "the branch moved"
    );
    assert_ne!(
        arm_source(files, Some(&lock("1111", "0.2.171"))),
        base,
        "a newer release resolved"
    );
    assert_ne!(
        arm_source(
            &files.replace("bbbb", "cccc"),
            Some(&lock("1111", "0.2.170"))
        ),
        base,
        "a file changed"
    );
    let instrument_moved = lock("1111", "0.2.170").replace("#aaaa", "#ffff");
    assert_eq!(
        arm_source(files, Some(&instrument_moved)),
        base,
        "the harness is not the code measured"
    );
}
