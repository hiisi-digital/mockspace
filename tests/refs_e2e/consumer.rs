//! Consumer repositories shaped like the ones that run v1 today, generated: a `mockspace.toml` at
//! the root naming the mock directory, design rounds of the dated naming, registry rows and
//! templates citing them in the spelling before the standard, presets in `lints.toml`, and the
//! directories a project keeps beside its sources. Every byte is made here; nothing is copied from a
//! real project.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::symlink;

use super::fixture::{CACHE_TAG, MANAGED_IGNORE, bytes, exec};
use super::world::{Clone, World};

/// What a consumer carries beside the shape every one of them has.
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub name:  &'static str,
    /// `mock_dir` of its `mockspace.toml`.
    pub mock:  &'static str,
    /// Whether it keeps research and benches, the directories a data ref takes.
    pub data:  bool,
    /// Files added to its research, for a mock directory of thousands.
    pub bulk:  usize,
    /// Symlinks, executables, binaries, CRLF, a name that is not UTF-8, and an LFS-like filter.
    pub odd:   bool,
    /// Whether it moves directories to data refs, or stays on the branch and takes the other
    /// migrations alone.
    pub refs:  bool,
}

/// Sources, rounds, templates and a registry, and no directory a data ref would take.
pub const PLAIN: Shape = Shape { name: "plain", mock: "mock", data: false, bulk: 0, odd: false, refs: false };
/// A mock directory of thousands of files.
pub const LARGE: Shape = Shape { name: "large", mock: "mock", data: true, bulk: 3000, odd: false, refs: true };
/// A mock directory that is not called `mock`.
pub const ELSEWHERE: Shape = Shape { name: "elsewhere", mock: "linux", data: true, bulk: 0, odd: false, refs: true };
/// Every kind of file git keeps that is not plain text.
pub const ODD: Shape = Shape { name: "odd", mock: "mock", data: true, bulk: 0, odd: true, refs: true };

pub const SHAPES: [Shape; 4] = [PLAIN, LARGE, ELSEWHERE, ODD];

/// A name that is not UTF-8: `caf\xe9.md`, as a Latin-1 tool writes it.
pub const NOT_UTF8: &[u8] = b"caf\xe9.md";

pub fn config(s: &Shape) -> String {
    format!(
        r#"# the consumer's config, at the root since a repo has exactly one
mockspace_branch = "dev"
project_name = "orchard"
crate_prefix = "orchard"
abi_version = 1
mock_dir = "{mock}"
proc_macro_crates = []
module_crates = []
layers = [
    "L0 core",
]

[domain_kinds]

[known_macros]

[ref.roots.seed]
path = "corpus"
frozen = true

[[registry.namespace]]
key = "ruling"
title = "Ruling"

[[registry.namespace.field]]
name = "provenance"
type = "ref[]"

[[registry.namespace.field]]
name = "why"
type = "string"

[lints.heap]
extends = "stack-lints::no-heap" # kept

[lints.numeric]
extends = "Mockspace::No-Bare-Numeric"
"#,
        mock = s.mock
    )
}

pub const LINTS: &str = "[lints.a]\nextends = \"stack-lints::no-heap\"\n\n[lints.b]\nextends = [\"Mockspace::No-Bare-Numeric\", \"stack-lints::9lives\"]\n";

pub const DESIGN: &str = "# Design\n\n## The four lanes\n\ntext\n\n## Warm's objective\n\nmore\n";

/// A template citing a row and a heading in the spelling before the standard, and one in a fence
/// that stays as written.
pub fn template(of: &str, eol: &str) -> String {
    ["# {of}", "", "See {{ seed::DESIGN::#the-four-lanes }} and {{ ruling::first }}.", "", "```", "{{ seed::DESIGN::12 }}", "```", ""]
        .join(eol)
        .replace("{of}", of)
}

pub fn rows(eol: &str) -> String {
    [
        "# rows of the consumer",
        "[[ruling]]",
        "id = \"first\"",
        "provenance = [\"seed::DESIGN::#the-four-lanes\", \"seed::DESIGN::3\"]",
        "why = \"see {{ seed::DESIGN::#warm-s-objective }}\"",
        "",
        "[[ruling]]",
        "id = \"second\"",
        "provenance = [\"seed::design::3\"]",
        "why = \"already in the new spelling\"",
        "",
    ]
    .join(eol)
}

impl Shape {
    /// `rel` under the mock directory, from the root.
    pub fn at(&self, rel: &str) -> String {
        format!("{}/{rel}", self.mock)
    }

    /// The `directory-to-ref` calls a consumer of this shape makes, in order, as `(namespace, path
    /// under the mock directory, include patterns)`.
    pub fn moves(&self) -> Vec<(&'static str, &'static str, &'static [&'static str])> {
        let mut out: Vec<(&str, &str, &[&str])> = Vec::new();
        if !self.refs {
            return out;
        }
        if self.data {
            out.push(("research", "research", &[]));
            out.push(("benches", "benches", &[]));
        }
        out.push(("rounds", "design_rounds", &["*/**"]));
        out.push(("templates", "crates", &["**/*.md.tmpl"]));
        out
    }

    /// The dated round files and the names `round-filenames` gives them, from the root.
    pub fn renames(&self) -> Vec<(String, String)> {
        [
            ("2026-03-07_alpha.md", "202603070000_topic.alpha.md"),
            ("2026-04-01_changelist.md", "202604010000_changelist.doc.md"),
        ]
        .iter()
        .map(|(f, t)| (self.at(&format!("design_rounds/{f}")), self.at(&format!("design_rounds/{t}"))))
        .collect()
    }

    /// Builds the consumer over several commits in `c` and pushes `dev`; ignored build output is on
    /// disk and never tracked.
    pub fn build(&self, c: &Clone) {
        let m = |rel: &str| self.at(rel);
        c.write(".gitignore", format!("{MANAGED_IGNORE}.env\n").as_bytes());
        c.write("mockspace.toml", config(self).as_bytes());
        c.write("lints.toml", LINTS.as_bytes());
        c.write("README.md", b"# orchard\n");
        c.write("LICENSE", b"a licence\n");
        c.write("corpus/DESIGN.md", DESIGN.as_bytes());
        c.write(&m("Cargo.toml"), b"[workspace]\nmembers = []\nresolver = \"2\"\n");
        c.write(&m("lints.toml"), LINTS.as_bytes());
        c.write(&m("DESIGN.md.tmpl"), template("design", "\n").as_bytes());
        for k in ["alpha", "beta"] {
            c.write(&m(&format!("crates/{k}/Cargo.toml")), format!("[package]\nname = \"{k}\"\nversion = \"0.1.0\"\n").as_bytes());
            c.write(&m(&format!("crates/{k}/src/lib.rs")), format!("pub fn {k}() {{}}\n").as_bytes());
            c.write(&m(&format!("crates/{k}/README.md.tmpl")), template(k, "\n").as_bytes());
            c.write(&m(&format!("crates/{k}/docs/GUIDE.md.tmpl")), format!("guide of {k}\n").as_bytes());
        }
        for k in ["tools/probe", "lints/shape"] {
            c.write(&m(&format!("{k}/Cargo.toml")), b"[package]\nname = \"x\"\nversion = \"0.1.0\"\n");
            c.write(&m(&format!("{k}/Cargo.lock")), b"# lock\nversion = 3\n");
            c.write(&m(&format!("{k}/src/main.rs")), b"fn main() {}\n");
        }
        c.write(&m("registry/ruling/rows.toml"), rows("\n").as_bytes());
        c.commit_all("chore: the consumer");

        c.write(&m("design_rounds/202609010000/202609010000_topic.closed.md"), b"a closed round\n");
        c.write(&m("design_rounds/202609010000/202609010000_changelist.doc.lock.md"), b"- one\n");
        c.write(&m("design_rounds/202610010000/202610010000_topic.later.md"), b"a later closed round\n");
        c.write(&m("design_rounds/2026-03-07_alpha.md"), b"an open topic of the dated naming\n");
        c.write(&m("design_rounds/2026-04-01_changelist.md"), b"- a changelist of the dated naming\n");
        c.write(&m("design_rounds/202611010000_topic.open.md"), b"an open topic of the compact naming\n");
        c.commit_all("docs: rounds");

        if self.data {
            c.write(&m("research/20261001_alpha/NOTES.md"), b"findings on alpha\n");
            c.write(&m("research/20261001_alpha/figure.png"), &bytes(1, 4096));
            c.write(&m("research/notes with spaces.md"), b"spaces in the name\n");
            c.write(&m("research/n\u{e4}kym\u{e4}/\u{e4}\u{e4}kk\u{f6}set.md"), "non-ascii path\n".as_bytes());
            c.write(&m("research/empty.txt"), b"");
            c.write(&m("benches/hot/bench.rs"), b"fn main() {}\n");
            c.write(&m("benches/hot/baseline.json"), b"{\"ns\": 120}\n");
            c.write(&m("benches/hot/run.sh"), b"#!/bin/sh\necho bench\n");
            exec(c, &m("benches/hot/run.sh"));
            for i in 0 .. self.bulk {
                c.write(&m(&format!("research/bulk/{:03}/f{i:05}.md", i % 200)), format!("file number {i}\n").repeat(4).as_bytes());
            }
            c.commit_all("docs: research and benches");
        }

        if self.odd {
            self.add_odd(c);
        }
        c.git(&["config", "gc.auto", "0"]);
        c.push();
        self.add_ignored_output(c);
    }

    fn add_odd(&self, c: &Clone) {
        let m = |rel: &str| self.at(rel);
        c.write(".gitattributes", b"*.lfs filter=lfslike -text\n");
        c.commit_all("chore: big files through the filter");
        c.write(&m("research/model.lfs"), &bytes(7, 200_000));
        c.write(&m("research/blob.bin"), &bytes(8, 70_000));
        c.write(&m("research/crlf.txt"), b"one\r\ntwo\r\n");
        c.write(&m("research/scripts/run.sh"), b"#!/bin/sh\necho research\n");
        exec(c, &m("research/scripts/run.sh"));
        let odd = c.mock().join("crates/alpha/src").join(OsStr::from_bytes(NOT_UTF8));
        std::fs::write(&odd, b"a name in latin-1\n").unwrap();
        c.write(&m("crates/alpha/CRLF.md.tmpl"), template("crlf", "\r\n").as_bytes());
        c.write(&m("registry/ruling/crlf.toml"), rows("\r\n").replace("first", "third").replace("second", "fourth").as_bytes());
        // a link the branch keeps beside its sources, under the directory the templates ref takes
        symlink("lib.rs", c.mock().join("crates/alpha/src/again.rs")).unwrap();
        c.commit_all("docs: files that are not plain text");
    }

    /// Build output inside the directories a migration moves, each `target` tagged as cargo tags it,
    /// and a secret a `.gitignore` keeps out.
    pub fn add_ignored_output(&self, c: &Clone) {
        let m = |rel: &str| self.at(rel);
        let mut targets = vec![m("target"), m("crates/alpha/target")];
        if self.data {
            targets.push(m("benches/hot/target"));
            c.write(&m("research/.env"), b"TOKEN=never-pushed\n");
        }
        for t in &targets {
            c.write(&format!("{t}/CACHEDIR.TAG"), CACHE_TAG);
            c.write(&format!("{t}/debug/out.o"), &bytes(4, 1024));
        }
    }
}

/// A remote holding a consumer of `shape` before any migration, built in a clone called `seed`.
pub fn remote(shape: &Shape) -> World {
    let w = World::new();
    shape.build(&w.first_clone("seed"));
    w
}

/// The flags of a `directory-to-ref` call.
pub fn move_args(ns: &str, path: &str, include: &[&str]) -> Vec<String> {
    super::fixture::migrate_args(ns, path, include)
}

/// Runs every migration as a consumer would, in order, committing what each says to commit, and
/// pushes. Returns nothing; the scenario checks what it needs.
pub fn migrate_all(shape: &Shape, c: &Clone) {
    migrate_until_references(shape, c);
    c.ok(&["migrate", "references-to-letkajenkka"]);
    c.commit_all("chore: rewrite references to the letkajenkka spelling");
    if shape.refs {
        c.ok(&["data-refs", "commit"]);
    }
    c.push();
}

/// The migrations before the reference one: the rounds renamed and each directory moved, each
/// committed as its run says.
pub fn migrate_until_references(shape: &Shape, c: &Clone) {
    if !shape.renames().iter().all(|(f, _)| !c.exists(f)) {
        c.ok(&["migrate", "round-filenames"]);
        c.commit_all("chore: rename the dated rounds");
    }
    for (ns, path, include) in shape.moves() {
        let args = move_args(ns, path, include);
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        c.ok(&refs);
        c.commit_all(&format!("chore: move `{ns}` to its ref"));
    }
}

/// A remote holding a consumer of `shape` with every migration made and pushed.
pub fn migrated(shape: &Shape) -> World {
    let w = remote(shape);
    {
        let c = w.clone_of("migrator");
        migrate_all(shape, &c);
    }
    w
}

/// The declarations a consumer of `shape` ends with, for the checks.
pub fn decls(shape: &Shape) -> Vec<super::check::Held> {
    shape.moves().into_iter().map(|(ns, path, _)| super::check::Held { ns, path }).collect()
}
