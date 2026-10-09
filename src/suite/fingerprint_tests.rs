//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The controls: each kind of input a test depends on, touched alone, moves
//! the fingerprints of exactly the members that read it.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;
use crate::suite::graph::{DepKind, Graph, Package};

/// A repository shaped like kaski's, small: `mesh` is a library, `render`
/// depends on it and keeps shaders beside its sources, `web` depends on
/// `render`, and `tool` depends on nothing. `render`'s tests name a content
/// file with `include_str!`, `web`'s name a content directory through the
/// manifest directory, and a second content file nothing names sits beside.
struct Repo {
    root: PathBuf,
}

impl Repo {
    fn new(tag: &str) -> Repo {
        let root = std::env::temp_dir().join(format!(
            "mockspace-suite-fp-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let r = Repo {
            root,
        };
        r.write("mock/Cargo.toml", "[workspace]\nmembers = []\n");
        r.write("mock/Cargo.lock", "# lock v1\n");
        r.write(
            "mock/crates/mesh/Cargo.toml",
            "[package]\nname = \"mesh\"\n",
        );
        r.write(
            "mock/crates/mesh/src/lib.rs",
            "pub fn quad() -> u32 { 4 }\n",
        );
        r.write(
            "mock/crates/render/Cargo.toml",
            "[package]\nname = \"render\"\n",
        );
        r.write("mock/crates/render/shaders/sprite.wgsl", "fn main() {}\n");
        r.write(
            "mock/crates/render/src/lib.rs",
            "// reads ../../../../content/kaski/unused.toml in a comment, which is not a read\n\
             const SHADER: &str = include_str!(\"../shaders/sprite.wgsl\");\n\
             const ROWS: &str = include_str!(\"../../../../content/kaski/rows.toml\");\n",
        );
        r.write("mock/crates/web/Cargo.toml", "[package]\nname = \"web\"\n");
        r.write(
            "mock/crates/web/src/lib.rs",
            "fn dir() -> std::path::PathBuf {\n\
             \x20   std::path::Path::new(env!(\"CARGO_MANIFEST_DIR\")).join(\"../../../content/kaski/interface\")\n\
             }\n",
        );
        r.write(
            "mock/crates/tool/Cargo.toml",
            "[package]\nname = \"tool\"\n",
        );
        r.write("mock/crates/tool/src/lib.rs", "pub fn t() {}\n");
        r.write("content/kaski/rows.toml", "a = 1\n");
        r.write("content/kaski/interface/theme.toml", "dark = true\n");
        r.write("content/kaski/unused.toml", "nobody = \"reads this\"\n");
        r.write("mock/vendor/lim/Cargo.toml", "[package]\nname = \"lim\"\n");
        r.write("mock/vendor/lim/src/lib.rs", "pub fn bind() {}\n");
        let ext = r.outside().join("ext");
        std::fs::create_dir_all(ext.join("src")).unwrap();
        std::fs::write(ext.join("src/lib.rs"), "pub fn e() {}\n").unwrap();
        std::fs::create_dir_all(ext.join("target")).unwrap();
        std::fs::write(ext.join("target/out"), "build output\n").unwrap();
        r.write(".gitignore", "target/\n*.log\n");
        r.git(&["init", "-q"]);
        r.git(&["add", "-A"]);
        r.git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "fixture"]);
        r
    }

    /// A directory beside the repository, for a path dependency outside it.
    fn outside(&self) -> PathBuf {
        self.root.with_extension("outside")
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

    fn graph(&self) -> Graph {
        let pkg = |name: &str, deps: &[(&str, DepKind)]| {
            let dir = match name {
                "lim" => self.root.join("mock/vendor/lim"),
                "ext" => self.outside().join("ext"),
                _ => self.root.join("mock/crates").join(name),
            };
            Package {
                name: name.to_string(),
                dir,
                deps: deps.iter().map(|(d, k)| (d.to_string(), *k)).collect(),
            }
        };
        let mut g = Graph::default();
        for p in [
            pkg("mesh", &[]),
            pkg("render", &[
                ("mesh", DepKind::Normal),
                ("lim", DepKind::Normal),
            ]),
            pkg("web", &[("render", DepKind::Normal)]),
            pkg("tool", &[("ext", DepKind::Normal)]),
            pkg("lim", &[]),
            pkg("ext", &[]),
        ] {
            g.packages.insert(p.name.clone(), p);
        }
        g.default_members = g.packages.keys().cloned().collect();
        g
    }

    fn fp(&self) -> Fingerprints {
        compute(&self.root, &self.root.join("mock"), &self.graph()).unwrap()
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
        std::fs::remove_dir_all(self.outside()).ok();
    }
}

/// Which members' fingerprints differ between two computations.
fn moved(a: &Fingerprints, b: &Fingerprints) -> Vec<String> {
    a.full
        .iter()
        .filter(|(k, v)| b.full.get(*k) != Some(*v))
        .map(|(k, _)| k.clone())
        .collect()
}

#[test]
fn nothing_touched_moves_nothing() {
    let r = Repo::new("none");
    assert_eq!(moved(&r.fp(), &r.fp()), Vec::<String>::new());
}

/// A dependency's source reaches everything compiled against it, and only that.
#[test]
fn a_dependencys_source_moves_it_and_its_dependents() {
    let r = Repo::new("dep");
    let before = r.fp();
    r.append("mock/crates/mesh/src/lib.rs", "pub fn tri() -> u32 { 3 }\n");
    assert_eq!(moved(&before, &r.fp()), vec!["mesh", "render", "web"]);
}

#[test]
fn a_members_own_source_moves_it_and_its_dependents_not_its_dependencies() {
    let r = Repo::new("own");
    let before = r.fp();
    r.append("mock/crates/render/src/lib.rs", "// one line\n");
    assert_eq!(moved(&before, &r.fp()), vec!["render", "web"]);
}

/// A shader kept beside the sources is one of the member's own files.
#[test]
fn a_shader_moves_the_member_that_keeps_it() {
    let r = Repo::new("shader");
    let before = r.fp();
    r.append("mock/crates/render/shaders/sprite.wgsl", "// tint\n");
    assert_eq!(moved(&before, &r.fp()), vec!["render", "web"]);
}

/// Content named with `include_str!` relative to the source file.
#[test]
fn content_a_source_includes_moves_that_member() {
    let r = Repo::new("include");
    let before = r.fp();
    r.append("content/kaski/rows.toml", "b = 2\n");
    assert_eq!(moved(&before, &r.fp()), vec!["render", "web"]);
}

/// Content named as a directory joined to `CARGO_MANIFEST_DIR`, read at run
/// time: any file under it is an input, a new one included.
#[test]
fn content_a_test_reads_at_run_time_moves_that_member() {
    let r = Repo::new("runtime");
    let before = r.fp();
    r.append("content/kaski/interface/theme.toml", "light = false\n");
    assert_eq!(moved(&before, &r.fp()), vec!["web"]);

    let before = r.fp();
    r.write("content/kaski/interface/forms.toml", "new = 1\n");
    assert_eq!(
        moved(&before, &r.fp()),
        vec!["web"],
        "an untracked new file is an input"
    );
}

/// The negative controls: content nobody names, a path only in a comment, and
/// an ignored build output move nothing.
#[test]
fn what_no_member_reads_moves_nothing() {
    let r = Repo::new("unread");
    let before = r.fp();
    r.append("content/kaski/unused.toml", "still = \"unread\"\n");
    r.write("mock/crates/render/run.log", "ignored by git\n");
    r.write("mock/target/debug/out", "ignored by git\n");
    assert_eq!(moved(&before, &r.fp()), Vec::<String>::new());
}

#[test]
fn the_lockfile_moves_every_package() {
    let r = Repo::new("lock");
    let before = r.fp();
    r.append("mock/Cargo.lock", "# bumped\n");
    assert_eq!(moved(&before, &r.fp()), vec![
        "ext", "lim", "mesh", "render", "tool", "web"
    ]);
}

/// A vendored crate is no member, and reaches what depends on it all the same.
#[test]
fn a_vendored_path_dependency_moves_its_dependents() {
    let r = Repo::new("vendor");
    let before = r.fp();
    r.append("mock/vendor/lim/src/lib.rs", "pub fn unbind() {}\n");
    assert_eq!(moved(&before, &r.fp()), vec!["lim", "render", "web"]);
}

/// A path dependency outside the repository is read off the disk, and its
/// build output is not an input.
#[test]
fn a_path_dependency_outside_the_repository_is_read_off_the_disk() {
    let r = Repo::new("outside");
    let before = r.fp();
    std::fs::write(r.outside().join("ext/target/out"), "rebuilt\n").unwrap();
    assert_eq!(moved(&before, &r.fp()), Vec::<String>::new());
    std::fs::write(r.outside().join("ext/src/lib.rs"), "pub fn e2() {}\n").unwrap();
    assert_eq!(moved(&before, &r.fp()), vec!["ext", "tool"]);
}

#[test]
fn the_report_names_what_each_member_reaches() {
    let r = Repo::new("reaches");
    let fp = r.fp();
    assert_eq!(fp.reaches["render"], vec![PathBuf::from(
        "content/kaski/rows.toml"
    )]);
    assert_eq!(fp.reaches["web"], vec![PathBuf::from(
        "content/kaski/interface"
    )]);
    assert!(fp.reaches["mesh"].is_empty());
}

#[test]
fn only_literals_that_climb_out_and_name_something_are_paths() {
    let src = r##"
        // "../not/a/literal/in/code"
        /// "../nor/in/a/doc"
        const A: &str = include_str!("../../content/a.toml");
        const B: &str = "/../../content/b";
        const C: &str = r#"../raw/c"#;
        const D: &str = "..";
        const E: &str = "../..";
        const F: &str = "shaders/inside.wgsl";
        const G: &str = "../escaped/\u{61}";
        const H: &str = "a/../b";
    "##;
    assert_eq!(path_literals(src), vec![
        "../../content/a.toml",
        "../raw/c",
        "/../../content/b",
    ]);
}

#[test]
fn a_literal_resolves_against_the_file_then_the_member() {
    let r = Repo::new("reach");
    let member = Path::new("mock/crates/render");
    let file_dir = Path::new("mock/crates/render/src");
    // From the source file, as `include_str!` reads it.
    assert_eq!(
        reach(
            "../../../../content/kaski/rows.toml",
            file_dir,
            member,
            &r.root
        ),
        Some(PathBuf::from("content/kaski/rows.toml"))
    );
    // From the member directory, as `CARGO_MANIFEST_DIR` reads it.
    assert_eq!(
        reach("/../../../content/kaski", file_dir, member, &r.root),
        Some(PathBuf::from("content/kaski"))
    );
    // Into itself, around itself, out of the repository, or to nothing.
    assert_eq!(reach("../shaders", file_dir, member, &r.root), None);
    assert_eq!(reach("../../render/src", file_dir, member, &r.root), None);
    assert_eq!(reach("../../crates", file_dir, member, &r.root), None);
    assert_eq!(
        reach("../../../../../../etc/passwd", file_dir, member, &r.root),
        None
    );
    assert_eq!(
        reach(
            "../../../../content/missing.toml",
            file_dir,
            member,
            &r.root
        ),
        None
    );
}
