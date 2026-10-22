//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The controls for telling a member's test binaries apart: each kind of file
//! touched alone moves exactly the digests that are built from it.
//!
//! The set of digests is the property. A fingerprint of a member and of each of
//! its binaries is asked of every kind of change, and the answer is the exact
//! list that moved, so a rule that moves too little (a test skipped that should
//! have run) and one that moves too much (the narrowing lost) both fail.

use std::path::PathBuf;
use std::process::Command;

use super::*;
use crate::suite::graph::{DepKind, Graph, Package};

/// `base` is a library with one integration test `x`. `uses` depends on it and
/// has: `one`, which reads `fix/a.json` and `content/one.toml`; `two`, which
/// declares `mod common`; `envy`, which reads the environment; a directory-form
/// target `dir`; and `data/note.txt`, which nothing names. `lone` has no tests.
struct Repo {
    root: PathBuf,
}

impl Repo {
    fn new(tag: &str) -> Repo {
        let root = std::env::temp_dir().join(format!(
            "mockspace-suite-units-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let r = Repo {
            root,
        };
        let manifest =
            |n: &str, dep: &str| format!("[package]\nname = \"{n}\"\n\n[dependencies]\n{dep}");
        r.write("mock/Cargo.toml", "[workspace]\nmembers = []\n");
        r.write("mock/Cargo.lock", "# lock v1\n");
        r.write("mock/crates/base/Cargo.toml", &manifest("base", ""));
        r.write(
            "mock/crates/base/src/lib.rs",
            "pub fn four() -> u32 { 4 }\n",
        );
        r.write("mock/crates/base/tests/x.rs", "#[test]\nfn x() {}\n");
        r.write(
            "mock/crates/uses/Cargo.toml",
            &manifest("uses", "base = { path = \"../base\" }\n"),
        );
        r.write("mock/crates/uses/src/lib.rs", "pub fn f() {}\n");
        r.write(
            "mock/crates/uses/tests/one.rs",
            "const A: &str = include_str!(\"fix/a.json\");\n\
             const C: &str = include_str!(\"../../../../content/one.toml\");\n",
        );
        r.write("mock/crates/uses/tests/fix/a.json", "{}\n");
        r.write("mock/crates/uses/tests/two.rs", "mod common;\n");
        r.write("mock/crates/uses/tests/common/mod.rs", "pub fn c() {}\n");
        r.write(
            "mock/crates/uses/tests/envy.rs",
            "fn dir() -> &'static str { env!(\"CARGO_MANIFEST_DIR\") }\n",
        );
        r.write("mock/crates/uses/tests/dir/main.rs", "mod part;\n");
        r.write("mock/crates/uses/tests/dir/part.rs", "");
        r.write("mock/crates/uses/tests/dir/golden.txt", "g\n");
        r.write(
            "mock/crates/uses/tests/data/note.txt",
            "nobody names this\n",
        );
        r.write("mock/crates/lone/Cargo.toml", &manifest("lone", ""));
        r.write("mock/crates/lone/src/lib.rs", "");
        r.write("content/one.toml", "a = 1\n");
        r.write("content/other.toml", "b = 2\n");
        r.write(".gitignore", "target/\n");
        r.git(&["init", "-q"]);
        r.git(&["add", "-A"]);
        r.git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "fixture"]);
        r
    }

    fn write(&self, rel: &str, text: &str) {
        let p = self.root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn append(&self, rel: &str, text: &str) {
        let p = self.root.join(rel);
        let mut t = std::fs::read_to_string(&p).unwrap();
        t.push_str(text);
        std::fs::write(p, t).unwrap();
    }

    fn git(&self, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    fn fp(&self) -> Fingerprints {
        let pkg = |name: &str, deps: &[&str]| {
            Package {
                name: name.to_string(),
                dir:  self.root.join("mock/crates").join(name),
                deps: deps
                    .iter()
                    .map(|d| (d.to_string(), DepKind::Normal))
                    .collect(),
            }
        };
        let mut g = Graph::default();
        for p in [pkg("base", &[]), pkg("uses", &["base"]), pkg("lone", &[])] {
            g.packages.insert(p.name.clone(), p);
        }
        g.default_members = g.packages.keys().cloned().collect();
        compute(&self.root, &self.root.join("mock"), &g, None).unwrap()
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

/// Every digest that differs between two computations, named by what it is:
/// `full:uses`, `shared:uses`, `base:uses`, `bin:uses::one`.
fn moved(a: &Fingerprints, b: &Fingerprints) -> Vec<String> {
    let mut out = Vec::new();
    for (label, x, y) in [
        ("own", &a.own, &b.own),
        ("shared", &a.shared, &b.shared),
        ("full", &a.full, &b.full),
        ("base", &a.base, &b.base),
        ("bin", &a.bins, &b.bins),
    ] {
        for (k, v) in x {
            if y.get(k) != Some(v) {
                out.push(format!("{label}:{k}"));
            }
        }
    }
    out
}

/// What moves when a member's every binary moves: the shared digest, the base,
/// and each of `uses`' four binaries.
const ALL_OF_USES: &[&str] = &[
    "base:uses",
    "bin:uses::dir",
    "bin:uses::envy",
    "bin:uses::one",
    "bin:uses::two",
    "full:uses",
    "own:uses",
    "shared:uses",
];

#[test]
fn the_binaries_of_a_member_are_told_apart() {
    let r = Repo::new("keys");
    let fp = r.fp();
    assert_eq!(fp.bins.keys().cloned().collect::<Vec<_>>(), vec![
        "base::x",
        "uses::dir",
        "uses::envy",
        "uses::one",
        "uses::two"
    ]);
    assert_eq!(fp.base.len(), 3);
    assert_eq!(
        moved(&fp, &r.fp()),
        Vec::<String>::new(),
        "nothing touched, nothing moved"
    );
}

/// The case the narrowing is for: one integration-test file, touched alone.
#[test]
fn a_change_confined_to_one_test_file_moves_that_binary_alone() {
    let r = Repo::new("one-file");
    let before = r.fp();
    r.append("mock/crates/uses/tests/one.rs", "// a line\n");
    let after = r.fp();
    // `envy` may read any claimed file, so it moves with it. `two` and `dir` do
    // not, nor the library, nor anything that depends on `uses`.
    assert_eq!(moved(&before, &after), vec![
        "own:uses",
        "full:uses",
        "bin:uses::envy",
        "bin:uses::one"
    ]);
    assert_eq!(before.shared["uses"], after.shared["uses"]);
    assert_eq!(before.base["uses"], after.base["uses"]);
}

#[test]
fn a_change_to_the_library_moves_every_binary_and_the_base() {
    let r = Repo::new("src");
    let before = r.fp();
    r.append("mock/crates/uses/src/lib.rs", "// a line\n");
    let mut m = moved(&before, &r.fp());
    m.sort();
    assert_eq!(m, ALL_OF_USES);
}

#[test]
fn a_file_no_binary_claims_is_shared_and_moves_them_all() {
    let r = Repo::new("unclaimed");
    let before = r.fp();
    r.append("mock/crates/uses/tests/data/note.txt", "x\n");
    let mut m = moved(&before, &r.fp());
    m.sort();
    assert_eq!(m, ALL_OF_USES);
}

#[test]
fn the_manifest_and_the_build_script_are_shared() {
    let r = Repo::new("manifest");
    let before = r.fp();
    r.append("mock/crates/uses/Cargo.toml", "\n# a comment\n");
    let mut m = moved(&before, &r.fp());
    m.sort();
    assert_eq!(m, ALL_OF_USES);

    let before = r.fp();
    r.write("mock/crates/uses/build.rs", "fn main() {}\n");
    let mut m = moved(&before, &r.fp());
    m.sort();
    assert_eq!(m, ALL_OF_USES, "a new build script is a new shared input");
}

/// A helper module belongs to the targets that declare it.
#[test]
fn a_helper_module_moves_the_targets_that_declare_it() {
    let r = Repo::new("helper");
    let before = r.fp();
    r.append("mock/crates/uses/tests/common/mod.rs", "// a line\n");
    assert_eq!(moved(&before, &r.fp()), vec![
        "own:uses",
        "full:uses",
        "bin:uses::envy",
        "bin:uses::two"
    ]);
}

/// A data file a test names is that test's, and the directory form owns its own.
#[test]
fn a_file_a_test_names_or_a_directory_target_owns_moves_that_binary() {
    let r = Repo::new("named");
    let before = r.fp();
    r.append("mock/crates/uses/tests/fix/a.json", "\n");
    assert_eq!(moved(&before, &r.fp()), vec![
        "own:uses",
        "full:uses",
        "bin:uses::envy",
        "bin:uses::one"
    ]);

    let before = r.fp();
    r.append("mock/crates/uses/tests/dir/golden.txt", "more\n");
    assert_eq!(moved(&before, &r.fp()), vec![
        "own:uses",
        "full:uses",
        "bin:uses::dir",
        "bin:uses::envy"
    ]);
}

/// Content outside the member that one test names moves that test, and content
/// the library names moves the library and so everything.
#[test]
fn content_read_from_outside_moves_the_binary_that_names_it() {
    let r = Repo::new("content");
    let before = r.fp();
    r.append("content/one.toml", "c = 3\n");
    assert_eq!(moved(&before, &r.fp()), vec![
        "own:uses",
        "full:uses",
        "bin:uses::envy",
        "bin:uses::one"
    ]);
    let before = r.fp();
    r.append("content/other.toml", "d = 4\n");
    assert_eq!(
        moved(&before, &r.fp()),
        Vec::<String>::new(),
        "nobody names it"
    );

    r.append(
        "mock/crates/uses/src/lib.rs",
        "pub const O: &str = include_str!(\"../../../../content/other.toml\");\n",
    );
    let before = r.fp();
    r.append("content/other.toml", "e = 5\n");
    let mut m = moved(&before, &r.fp());
    m.sort();
    assert_eq!(m, ALL_OF_USES);
}

/// What a dependent is compiled against is the library, never the tests.
#[test]
fn a_dependencys_tests_do_not_move_its_dependents_but_its_library_does() {
    let r = Repo::new("dependents");
    let before = r.fp();
    r.append("mock/crates/base/tests/x.rs", "// a line\n");
    assert_eq!(
        moved(&before, &r.fp()),
        vec!["own:base", "full:base", "bin:base::x"],
        "`uses` is not compiled from base's tests"
    );

    let before = r.fp();
    r.append("mock/crates/base/src/lib.rs", "// a line\n");
    let mut m = moved(&before, &r.fp());
    m.sort();
    let mut want: Vec<&str> = vec![
        "own:base",
        "shared:base",
        "full:base",
        "base:base",
        "bin:base::x",
        "full:uses",
        "base:uses",
    ];
    want.extend(["bin:uses::dir", "bin:uses::envy", "bin:uses::one", "bin:uses::two"]);
    want.sort();
    assert_eq!(
        m, want,
        "own:uses and shared:uses stay put: only the closure moved"
    );
}

/// Every file a build reads whatever it builds, read from the mock directory and
/// from the repository root, moves every digest of every member, a file that is
/// new and one that was there alike; one beside them that is not on the list
/// moves none.
#[test]
fn the_workspace_moving_moves_every_digest_of_every_member() {
    let digests = |fp: &Fingerprints| {
        let mut all: Vec<(String, String)> = Vec::new();
        for (label, m) in [("full", &fp.full), ("base", &fp.base), ("bin", &fp.bins)] {
            all.extend(m.iter().map(|(k, v)| (format!("{label}:{k}"), v.clone())));
        }
        all
    };
    assert_eq!(
        WORKSPACE_INPUTS.len(),
        7,
        "a new workspace input needs its place here"
    );
    for base in ["mock", ""] {
        for name in WORKSPACE_INPUTS {
            let r = Repo::new(&format!("ws-{}", name.replace(['.', '/'], "-")));
            let rel = format!("{base}/{name}");
            let rel = rel.trim_start_matches('/');
            let before = digests(&r.fp());
            if r.root.join(rel).is_file() {
                r.append(rel, "# moved\n");
            } else {
                r.write(rel, "# new\n");
            }
            let after = digests(&r.fp());
            assert_eq!(before.len(), after.len());
            for ((k, a), (_, b)) in before.iter().zip(&after) {
                assert_ne!(a, b, "{rel} did not move {k}");
            }
        }
    }
    let r = Repo::new("ws-control");
    let before = digests(&r.fp());
    r.write("mock/.cargo/not-config.toml", "x = 1\n");
    r.write("NOTES.md", "text\n");
    assert_eq!(before, digests(&r.fp()));
}

/// Whatever is not an integration test shares the member's base, and an
/// integration test is its own.
#[test]
fn a_binary_is_its_own_or_its_members_base() {
    let r = Repo::new("of-binary");
    let fp = r.fp();
    assert_eq!(fp.of_binary("uses::one"), fp.bins.get("uses::one"));
    assert_eq!(fp.of_binary("uses"), fp.base.get("uses"));
    assert_eq!(fp.of_binary("uses::bin/tool"), fp.base.get("uses"));
    assert_eq!(fp.of_binary("uses::example/demo"), fp.base.get("uses"));
    assert_eq!(fp.of_binary("nobody::x"), None);
}

/// A member whose manifest says something this cannot narrow by is not
/// narrowed: every file is shared, so every binary moves with every file.
#[test]
fn a_manifest_that_cannot_be_narrowed_leaves_every_file_shared() {
    let r = Repo::new("unnarrowed");
    r.append(
        "mock/crates/uses/Cargo.toml",
        "\n[[test]]\nname = \"elsewhere\"\npath = \"src/elsewhere.rs\"\n",
    );
    r.write("mock/crates/uses/src/elsewhere.rs", "");
    let before = r.fp();
    assert!(
        before.bins.keys().all(|k| !k.starts_with("uses::")),
        "no binaries are told apart"
    );
    r.append("mock/crates/uses/tests/one.rs", "// a line\n");
    let mut m = moved(&before, &r.fp());
    m.sort();
    assert_eq!(m, ["base:uses", "full:uses", "own:uses", "shared:uses"]);
}

/// A file under `tests/` that one binary names is that binary's alone, until the
/// library names it too, and then every binary is built from it.
#[test]
fn a_claimed_file_the_library_names_moves_every_binary() {
    let r = Repo::new("pulled");
    r.append(
        "mock/crates/uses/src/lib.rs",
        "pub const A: &str = include_str!(\"../tests/fix/a.json\");\n",
    );
    let before = r.fp();
    r.append("mock/crates/uses/tests/fix/a.json", "\n");
    let mut m = moved(&before, &r.fp());
    m.sort();
    assert_eq!(m, ALL_OF_USES);
}

// ---- probes: inputs a binary depends on that the split does not see ----

/// The library compiles `tests/common/mod.rs` by `#[path]`, and that file
/// declares `inner`. `inner.rs` is built into the library, so every binary and
/// the base move with it. Only the direct literal is a shared pull.
#[test]
fn probe_a_module_the_library_pulls_from_tests_brings_its_own_modules() {
    let r = Repo::new("probe-lib-pulls-tree");
    r.write(
        "mock/crates/uses/tests/common/mod.rs",
        "mod inner;\npub fn c() {}\n",
    );
    r.write("mock/crates/uses/tests/common/inner.rs", "pub fn i() {}\n");
    r.append(
        "mock/crates/uses/src/lib.rs",
        "#[cfg(test)]\n#[path = \"../tests/common/mod.rs\"]\nmod common;\n",
    );
    let before = r.fp();
    r.append("mock/crates/uses/tests/common/inner.rs", "// edit\n");
    let mut m = moved(&before, &r.fp());
    m.sort();
    assert_eq!(m, ALL_OF_USES);
}

/// `include_dir!("$CARGO_MANIFEST_DIR/tests/dir")` and `rust-embed`'s `folder`
/// expand the placeholder themselves, and the test source mentions no `env!`.
#[test]
fn probe_a_placeholder_rooted_literal_names_what_it_expands_to() {
    let r = Repo::new("probe-placeholder");
    r.append(
        "mock/crates/uses/tests/one.rs",
        "const D: &str = \"$CARGO_MANIFEST_DIR/tests/dir/golden.txt\";\n",
    );
    let before = r.fp();
    r.append("mock/crates/uses/tests/dir/golden.txt", "more\n");
    let m = moved(&before, &r.fp());
    assert!(m.contains(&"bin:uses::one".to_string()), "{m:?}");
}

/// The same pull, with the content reached from the pulled file rather than a
/// module: the library compiles it, so the library's digests must move.
#[test]
fn probe_content_a_module_the_library_pulls_reads_moves_the_library() {
    let r = Repo::new("probe-lib-pulls-content");
    r.write(
        "mock/crates/uses/tests/common/mod.rs",
        "pub const O: &str = include_str!(\"../../../../../content/other.toml\");\n",
    );
    r.append(
        "mock/crates/uses/src/lib.rs",
        "#[cfg(test)]\n#[path = \"../tests/common/mod.rs\"]\nmod common;\n",
    );
    let before = r.fp();
    r.append("content/other.toml", "e = 5\n");
    let mut m = moved(&before, &r.fp());
    m.sort();
    assert_eq!(m, ALL_OF_USES);
}

#[test]
fn control_concat_include_bytes_and_custom_test_path_and_out_dir() {
    let r = Repo::new("control");
    // concat! of two literals
    r.write(
        "mock/crates/uses/tests/cc.rs",
        "const B: &[u8] = include_bytes!(concat!(\"fix/\", \"a.json\"));\n",
    );
    // custom path inside tests/, harness = false
    r.append(
        "mock/crates/uses/Cargo.toml",
        "\n[[test]]\nname = \"custom\"\npath = \"tests/sub/entry.rs\"\nharness = false\n",
    );
    r.write(
        "mock/crates/uses/tests/sub/entry.rs",
        "mod helper;\nfn main() {}\n",
    );
    r.write("mock/crates/uses/tests/sub/helper.rs", "");
    // OUT_DIR reader
    r.write(
        "mock/crates/uses/tests/od.rs",
        "const G: &str = include_str!(concat!(env!(\"OUT_DIR\"), \"/g.txt\"));\n",
    );
    r.write("mock/crates/uses/build.rs", "fn main() {}\n");
    let b = r.fp();
    assert!(
        b.bins.contains_key("uses::custom") && b.bins.contains_key("uses::cc"),
        "{:?}",
        b.bins.keys().collect::<Vec<_>>()
    );
    r.append("mock/crates/uses/tests/fix/a.json", "\n");
    let m = moved(&b, &r.fp());
    assert!(m.contains(&"bin:uses::cc".to_string()), "{m:?}");
    let b = r.fp();
    r.append("mock/crates/uses/tests/sub/helper.rs", "// x\n");
    let m = moved(&b, &r.fp());
    assert!(
        m.contains(&"bin:uses::custom".to_string())
            && !m.contains(&"bin:uses::cc".to_string())
            && !m.contains(&"shared:uses".to_string()),
        "{m:?}"
    );
    let b = r.fp();
    r.append("mock/crates/uses/build.rs", "// x\n");
    let m = moved(&b, &r.fp());
    assert!(
        m.contains(&"bin:uses::od".to_string()) && m.contains(&"shared:uses".to_string()),
        "{m:?}"
    );
}
