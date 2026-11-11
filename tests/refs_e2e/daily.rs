//! The days of a consumer after its migration: the shape every scenario of them starts from, a
//! person's clone of it, and the check every one ends on.

use super::consumer::{self, Shape};
use super::whole::Ledger;
use super::world::{Clone, World};

/// Links, executables, binaries, CRLF, a name that is not UTF-8 and a filtered file, all moved.
pub const S: Shape = consumer::ODD;
pub const NOTES: &str = "mock/research/20261001_alpha/NOTES.md";
pub const TMPL: &str = "mock/crates/alpha/README.md.tmpl";
pub const SRC: &str = "mock/crates/alpha/src/lib.rs";
pub const ROUND: &str = "mock/design_rounds/202609010000/202609010000_topic.closed.md";

/// The remote of a consumer with every migration made, and the `dev` commit the migration ends at.
pub fn migrated() -> (World, String) {
    let w = consumer::migrated(&S);
    let tip = w.remote_tip("refs/heads/dev").unwrap();
    (w, tip)
}

/// A person's clone of `w`: every ref instantiated, build output beside the sources, the engine's
/// first run made.
pub fn seat<'w>(w: &'w World, name: &str) -> Clone<'w> {
    let c = w.clone_of(name);
    c.ok(&["data-refs", "instantiate"]);
    S.add_ignored_output(&c);
    let _ = c.run(&["status"]);
    c
}

/// The files a ref holds that a commit of `c` after `base` tracks on the branch.
pub fn wrongly_held(c: &Clone, base: &str) -> Vec<String> {
    let held = |p: &str| {
        ["mock/research/", "mock/benches/", "mock/design_rounds/202609010000/", "mock/design_rounds/202610010000/"]
            .iter()
            .any(|pre| p.starts_with(pre))
            || (p.starts_with("mock/crates/") && p.ends_with(".md.tmpl"))
    };
    let mut out = Vec::new();
    for commit in c.git(&["rev-list", &format!("{base}..HEAD")]).lines() {
        let listing = c.git(&["ls-tree", "-r", "-z", "--name-only", commit]);
        out.extend(listing.split('\0').filter(|p| held(p)).map(|p| format!("{commit}: {p}")));
    }
    out
}

/// What every scenario ends on.
pub fn nothing_lost_or_wrongly_committed(w: &World, l: &Ledger, base: &str, clones: &[&Clone]) {
    l.kept(w, clones).unwrap();
    for c in clones {
        let bad = wrongly_held(c, base);
        assert!(bad.is_empty(), "committed to the branch though a ref holds it:\n{}", bad.join("\n"));
    }
}

/// The last `dev` commit from before the migration.
pub fn before_the_migration(c: &Clone) -> String {
    let pre = c.git(&["rev-list", "-1", "--grep=^docs: files that are not plain text", "dev"]).trim().to_string();
    assert!(!pre.is_empty(), "the control: the consumer's last commit before the migration is found");
    pre
}

