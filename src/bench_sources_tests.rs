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

fn put(root: &Path, files: &[(&str, &str)]) {
    for (path, body) in files {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
}

/// The text of the one tracking arm under `root`, as `mock bench run` takes
/// it, and as the lint pack alone reads it.
fn texts(root: &Path) -> (String, String) {
    let reading = Reading::new(root);
    let view = reading.view();
    let bench_dir = root.join("mock/benches");
    let code = bench_arm_code(&view, &bench_dir).pop().unwrap();
    let manifest = bench_dir.join(&code.crate_dir).join("Cargo.toml");
    (
        arm_text(&view, &bench_dir, &code, &manifest).unwrap(),
        code.text(&view, &bench_dir, &std::collections::BTreeSet::new()),
    )
}

/// A tracking arm's source follows a crate the living code takes through the
/// workspace it inherits from, the way hilavitkutin spells its siblings: the
/// arm takes `engine` by path, `engine` takes `kit` by `kit.workspace = true`,
/// and the root manifest declares where `kit` is. An edit to `kit` moves the
/// arm, which the lint pack's own reading does not see; an edit to the root
/// manifest moves it; an edit to a test of `kit` does not.
#[test]
fn a_crate_inherited_from_a_workspace_is_part_of_the_arm() {
    let tmp = mockspace_manifest::scratch::tempdir().unwrap();
    let root = tmp.path();
    put(root, &[
        ("mock/Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"2\"\n\n[workspace.dependencies]\nkit = { path = \"crates/kit\" }\n"),
        ("mock/crates/engine/Cargo.toml", "[package]\nname = \"engine\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[dependencies]\nkit.workspace = true\n"),
        ("mock/crates/engine/src/lib.rs", "pub use kit::g;\n"),
        ("mock/crates/kit/Cargo.toml", "[package]\nname = \"kit\"\nversion = \"0.0.0\"\nedition = \"2021\"\n"),
        ("mock/crates/kit/src/lib.rs", "pub fn g() {}\n"),
        ("mock/crates/kit/tests/t.rs", ""),
        ("mock/benches/bench.toml", "[timing]\n"),
        ("mock/benches/live/bench.toml", "kind = \"tracking\"\ntitle = \"l\"\narms = [\"a\"]\npoints = [64]\n"),
        ("mock/benches/live/arms/a/Cargo.toml", "[workspace]\n[package]\nname = \"a\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[lib]\ncrate-type = [\"cdylib\"]\n[dependencies]\nengine = { path = \"../../../../crates/engine\" }\n"),
        ("mock/benches/live/arms/a/src/lib.rs", ""),
    ]);
    let (first, lint_first) = texts(root);
    for p in ["../crates/kit/src/lib.rs", "../crates/kit/Cargo.toml", "../Cargo.toml", "../crates/engine/src/lib.rs"] {
        assert!(first.contains(&format!("  {p}\n")), "missing {p}:\n{first}");
    }
    assert!(!first.contains("tests/t.rs"), "{first}");

    put(root, &[("mock/crates/kit/tests/t.rs", "#[test] fn t() {}\n")]);
    assert_eq!(texts(root).0, first, "a test no arm builds moves nothing");

    put(root, &[("mock/crates/kit/src/lib.rs", "pub fn g() { loop {} }\n")]);
    let (after, lint_after) = texts(root);
    assert_ne!(after, first, "an edit to the inherited crate moves the arm");
    assert_eq!(lint_after, lint_first, "the case: the manifests alone do not reach it");

    put(root, &[("mock/Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"2\"\n\n[workspace.dependencies]\nkit = { path = \"crates/kit\" }\n\n[profile.release]\nopt-level = 1\n")]);
    assert_ne!(texts(root).0, after, "the root manifest it inherits from moves it");
}

/// What `cargo metadata` names a path package is one with no source, and the
/// walk takes those reached from the arm without passing through the
/// instrument, never the arm itself.
#[test]
fn path_crates_are_those_the_arm_reaches_past_the_instrument() {
    let meta = serde_json::json!({
        "packages": [
            {"id": "a", "name": "a", "source": null, "manifest_path": "/t/a/Cargo.toml"},
            {"id": "e", "name": "engine", "source": null, "manifest_path": "/t/e/Cargo.toml"},
            {"id": "l", "name": "libc", "source": "registry+x", "manifest_path": "/r/libc/Cargo.toml"},
            {"id": "c", "name": "mockspace-bench-core", "source": null, "manifest_path": "/m/core/Cargo.toml"},
            {"id": "h", "name": "mockspace-manifest", "source": null, "manifest_path": "/m/manifest/Cargo.toml"}
        ],
        "resolve": {"root": "a", "nodes": [
            {"id": "a", "deps": [{"pkg": "e"}, {"pkg": "c"}]},
            {"id": "e", "deps": [{"pkg": "l"}]},
            {"id": "c", "deps": [{"pkg": "h"}]}
        ]}
    });
    assert_eq!(path_crates_of(&meta).unwrap(), [PathBuf::from("/t/e")]);
}
