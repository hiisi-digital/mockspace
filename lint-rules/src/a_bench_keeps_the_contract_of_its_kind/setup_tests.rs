//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::tests::{ARM, findings, freeze, frozen_snapshot, repo, write};

/// The setup a snapshot runs under from the root `bench.toml` is frozen with
/// it, value by value: a changed `[timing]`, a `[build]` opt level and the
/// workload it names each move it and are each named, while an edit to a part
/// of the root file it does not use (another workload, the instrument's spec)
/// moves nothing.
#[test]
fn a_snapshot_s_lock_freezes_the_setup_it_uses_and_nothing_else() {
    let tmp = frozen_snapshot();
    write(&tmp, &[(
        "mock/benches/hash/bench.toml",
        "kind = \"snapshot\"\ntitle = \"h\"\nworkload = \"w\"\narms = [\"a\"]\npoints = [64]\n",
    )]);
    write(&tmp, &[(
        "mock/benches/bench.toml",
        "[timing]\n[build]\nopt-level = 3\n[workload.w]\nstages = [\"algo_call\"]\n",
    )]);
    freeze(&tmp, "hash");
    assert_eq!(findings(&tmp), Vec::<String>::new());

    write(&tmp, &[(
        "mock/benches/bench.toml",
        "[timing]\n[build]\nopt-level = 3\nmockspace = '{ path = \"x\" }'\n[workload.w]\nstages = [\"algo_call\"]\n[workload.other]\nstages = []\n",
    )]);
    assert_eq!(findings(&tmp), Vec::<String>::new(), "the root moved where the bench does not read");

    for (root, named) in [
        ("[timing]\npasses = 3\n[build]\nopt-level = 3\n[workload.w]\nstages = [\"algo_call\"]\n", "setup:[timing] changed"),
        ("[timing]\n[build]\nopt-level = 1\n[workload.w]\nstages = [\"algo_call\"]\n", "setup:[build] changed"),
        ("[timing]\n[build]\nopt-level = 3\n[workload.w]\nstages = [\"algo_call\", \"scalar_work 4\"]\n", "setup:[workload.w] changed"),
    ] {
        write(&tmp, &[("mock/benches/bench.toml", root)]);
        let f = findings(&tmp);
        assert_eq!(f.len(), 1, "{root}: {f:#?}");
        assert!(f[0].contains(named), "{named}: {}", f[0]);
    }
}

/// A root `[bench.<name>]` section locks its own section and the setup, never
/// the whole root file the other sections share.
#[test]
fn a_root_section_locks_its_own_section_not_the_root_file() {
    let tmp = repo(&[]);
    let root = |a: &str, b: &str| {
        format!(
            "[bench.one]\nkind = \"snapshot\"\ntitle = \"{a}\"\nworkload = \"default\"\nvariants = [\"fnv\"]\nsizes = [64]\n\
             [bench.two]\nkind = \"snapshot\"\ntitle = \"{b}\"\nworkload = \"default\"\nvariants = [\"fnv\"]\nsizes = [64]\n"
        )
    };
    write(&tmp, &[
        ("mock/benches/bench.toml", root("1", "2").as_str()),
        ("mock/benches/variants/fnv/Cargo.toml", ARM),
        ("mock/benches/variants/fnv/src/lib.rs", ""),
    ]);
    freeze(&tmp, "one");
    freeze(&tmp, "two");
    assert_eq!(findings(&tmp), Vec::<String>::new());
    write(&tmp, &[("mock/benches/bench.toml", root("1", "changed").as_str())]);
    let f = findings(&tmp);
    assert_eq!(f.len(), 1, "only the section that changed: {f:#?}");
    assert!(f[0].contains("snapshot `two`") && f[0].contains("setup:[bench.two] changed"), "{}", f[0]);
}

