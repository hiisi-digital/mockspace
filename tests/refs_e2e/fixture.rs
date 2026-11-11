//! A consumer repository shaped like a real one, generated deterministically: no checked-in data.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};

use super::world::Clone;

/// `len` bytes from a fixed generator, so a binary file is the same on every run and every host.
pub fn bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    (0..len)
        .map(|_| {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (s >> 33) as u8
        })
        .collect()
}

pub fn exec(c: &Clone, rel: &str) {
    let p = c.path(rel);
    let mut perm = fs::metadata(&p).unwrap().permissions();
    perm.set_mode(0o755);
    fs::set_permissions(p, perm).unwrap();
}

pub const MANAGED_IGNORE: &str = "# === mockspace-managed build artifacts (do not edit) ===\n# Catch-all: every cargo build dir, including nested ones under benches/,\n# tests/, and research sketches. A leading-slash /target would miss those.\ntarget/\n# === end mockspace-managed build artifacts ===\n";

/// The declarations the full migration of the fixture makes, as `(namespace, path, include)`.
pub const REFS: [(&str, &str, &[&str]); 4] = [
    ("research", "research", &[]),
    ("benches", "benches", &[]),
    ("rounds", "design_rounds", &["*/**"]),
    ("templates", "crates", &["**/*.md.tmpl"]),
];

/// Build the consumer in `c` over several commits and push `dev`.
pub fn build(c: &Clone) {
    // commit 1: the skeleton and the sources
    c.write(".gitignore", MANAGED_IGNORE.as_bytes());
    c.write("mock/mockspace.toml", b"project_name = \"fixture\"\ncrate_prefix = \"fixture\"\n");
    c.write("mock/Cargo.toml", b"[workspace]\nmembers = []\nresolver = \"2\"\n");
    c.write("README.md", b"# fixture\n");
    for k in ["alpha", "beta"] {
        c.write(&format!("mock/crates/{k}/Cargo.toml"), format!("[package]\nname = \"{k}\"\nversion = \"0.1.0\"\n").as_bytes());
        c.write(&format!("mock/crates/{k}/src/lib.rs"), format!("pub fn {k}() {{}}\n").as_bytes());
        c.write(&format!("mock/crates/{k}/README.md.tmpl"), format!("readme of {k}\n").as_bytes());
        c.write(&format!("mock/crates/{k}/docs/GUIDE.md.tmpl"), format!("guide of {k}\n").as_bytes());
    }
    c.write("mock/ROOT.md.tmpl", b"the root template\n");
    c.write("mock/OVERVIEW.md.tmpl", b"the overview template\n");
    c.commit_all("chore: skeleton");

    // commit 2: design rounds, closed and active
    c.write("mock/design_rounds/202609010000/topic.md", b"closed round one\n");
    c.write("mock/design_rounds/202609010000/changelist.md", b"- one\n- two\n");
    c.write("mock/design_rounds/202610010000/topic.md", b"closed round two\n");
    c.write("mock/design_rounds/202611010000_topic.md", b"the active round, a flat file\n");
    c.commit_all("docs: rounds");

    // commit 3: research with binaries, odd names, an empty file, a large file
    c.write("mock/research/20261001_alpha/NOTES.md", b"findings on alpha\n");
    c.write("mock/research/20261001_alpha/figure.png", &bytes(1, 4096));
    c.write("mock/research/20261001_alpha/data.bin", &bytes(2, 65537));
    c.write("mock/research/notes with spaces.md", b"spaces in the name\n");
    c.write("mock/research/n\u{e4}kym\u{e4}/\u{e4}\u{e4}kk\u{f6}set.md", "non-ascii path\n".as_bytes());
    c.write("mock/research/\u{65e5}\u{672c}\u{8a9e}.md", "wide characters\n".as_bytes());
    c.write("mock/research/empty.txt", b"");
    c.write("mock/research/large.bin", &bytes(3, 3 * 1024 * 1024));
    c.write("mock/research/crlf.txt", b"one\r\ntwo\r\n");
    c.write("mock/research/sketches/probe/src/main.rs", b"fn main() {}\n");
    c.commit_all("docs: research");

    // commit 4: benches, tools and lints with their own lockfiles, executables
    c.write("mock/benches/hot/bench.rs", b"fn main() {}\n");
    c.write("mock/benches/hot/baseline.json", b"{\"ns\": 120}\n");
    c.write("mock/benches/hot/run.sh", b"#!/bin/sh\necho bench\n");
    exec(c, "mock/benches/hot/run.sh");
    for k in ["tools/probe", "lints/shape"] {
        c.write(&format!("mock/{k}/Cargo.toml"), b"[package]\nname = \"x\"\nversion = \"0.1.0\"\n");
        c.write(&format!("mock/{k}/Cargo.lock"), b"# lock\nversion = 3\n");
        c.write(&format!("mock/{k}/src/main.rs"), b"fn main() {}\n");
    }
    exec(c, "mock/tools/probe/src/main.rs");
    c.commit_all("feat: benches, tools and lints");

    // commit 5: an edit, so a file has more than one version
    c.write("mock/research/20261001_alpha/NOTES.md", b"findings on alpha, revised\n");
    c.commit_all("docs: revise the alpha notes");

    // ignored build output, nested, never tracked
    c.write("mock/target/debug/junk.o", &bytes(4, 2048));
    c.write("mock/benches/hot/target/release/out.bin", &bytes(5, 2048));
    c.write("mock/research/sketches/probe/target/debug/x", &bytes(6, 512));

    c.push();
}

/// The flags of one `directory-to-ref` call.
pub fn migrate_args(ns: &str, path: &str, include: &[&str]) -> Vec<String> {
    let mut a: Vec<String> =
        ["migrate", "directory-to-ref", "--namespace", ns, "--path", path].iter().map(|s| s.to_string()).collect();
    for i in include {
        a.push("--include".into());
        a.push((*i).into());
    }
    a
}

/// Run one migration through the entry and commit its config and removals, as the guide says.
pub fn migrate_one(c: &Clone, ns: &str, path: &str, include: &[&str]) {
    let args = migrate_args(ns, path, include);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    c.ok(&refs);
    c.git(&["add", "-f", "--", "mock/mockspace.toml"]);
    c.commit_all(&format!("chore: move `{ns}` to its ref"));
}

/// The four migrations of the fixture, in order.
pub fn migrate_all(c: &Clone) {
    for (ns, path, include) in REFS {
        migrate_one(c, ns, path, include);
    }
}

pub fn decls() -> Vec<super::check::Held> {
    REFS.iter().map(|(ns, path, _)| super::check::Held { ns, path }).collect()
}

/// Track a symlink among the research files, and commit it.
pub fn add_research_symlink(c: &Clone) {
    symlink("20261001_alpha/NOTES.md", c.path("mock/research/latest")).unwrap();
    c.commit_all("docs: link the latest notes");
}

/// A remote holding the migrated fixture: `dev` carries the config, the refs carry the data.
pub fn migrated_remote() -> super::world::World {
    let w = super::world::World::new();
    {
        let seed = w.first_clone("seed");
        build(&seed);
        migrate_all(&seed);
        seed.push();
    }
    w
}

/// A clone of the migrated remote with its instances in place.
pub fn seat<'w>(w: &'w super::world::World, name: &str) -> Clone<'w> {
    let c = w.clone_of(name);
    c.ok(&["data-refs", "instantiate"]);
    c
}
