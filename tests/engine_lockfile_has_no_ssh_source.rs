//! The launcher installs the engine anonymously from the public repository, so no package in
//! the engine's lockfiles may come from a `git+ssh` source: a fetch over ssh asks for a credential
//! nobody supplies.

use std::path::Path;

/// Every `(package, source)` in `lock` whose source is fetched over ssh.
fn ssh_sources(lock: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut name = String::new();
    for line in lock.lines() {
        if let Some(n) = line.strip_prefix("name = \"") {
            name = n.trim_end_matches('"').to_owned();
        } else if let Some(s) = line.strip_prefix("source = \"") {
            let s = s.trim_end_matches('"');
            if s.starts_with("git+ssh://") {
                found.push((name.clone(), s.to_owned()));
            }
        }
    }
    found
}

#[test]
fn the_finder_names_an_ssh_source_and_passes_the_rest() {
    // The case that must fail: the finder has to see a `git+ssh` source at all.
    let lock = "[[package]]\nname = \"a\"\nsource = \"git+ssh://git@example.invalid/a.git#abc\"\n\n\
                [[package]]\nname = \"b\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n\n\
                [[package]]\nname = \"c\"\nsource = \"git+https://example.invalid/c.git#def\"\n";
    let found = ssh_sources(lock);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "a");
}

#[test]
fn no_engine_lockfile_pulls_a_package_over_ssh() {
    // FIXME(02b7c73b2e63): red until `letkajenkka` is a crates.io version; waits on the rc publish.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut offenders = Vec::new();
    for lock in ["Cargo.lock", "mock/Cargo.lock"] {
        let text = std::fs::read_to_string(root.join(lock)).unwrap_or_else(|e| panic!("{lock}: {e}"));
        for (name, source) in ssh_sources(&text) {
            offenders.push(format!("{lock}: {name} from {source}"));
        }
    }
    assert!(offenders.is_empty(), "the launcher installs anonymously: {offenders:#?}");
}
