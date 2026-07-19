#![allow(unused_imports)]
use super::*;

#[test]
fn re_pinning_discards_the_built_binary() {
    // The stale binary is the invisible half of a stale pin: the manifest
    // names one revision and the running code comes from another, so a
    // landed fix appears not to work.
    let tmp = tempfile::tempdir().unwrap();
    let proxy = tmp.path().join("mockspace-proxy");
    let debug = proxy.join("target").join("debug");
    fs::create_dir_all(&debug).unwrap();
    let bin = debug.join("mockspace-proxy");
    fs::write(&bin, b"stale").unwrap();
    // A sibling artifact stands in for the dependency compilation that is
    // still valid: removing it would make every re-pin a full rebuild.
    let dep = debug.join("libmockspace.rlib");
    fs::write(&dep, b"deps").unwrap();

    let mut actions = Vec::new();
    discard_proxy_binary(&proxy, &mut actions);

    assert!(!bin.exists(), "the built proxy survived a re-pin");
    assert!(dep.exists(), "unrelated build output was removed");
    assert_eq!(actions.len(), 1, "{actions:?}");
}

#[test]
fn discarding_is_quiet_when_there_is_nothing_built() {
    // A first run has no binary. Reporting a discard that did not happen
    // would make the common case look like a recovery.
    let tmp = tempfile::tempdir().unwrap();
    let mut actions = Vec::new();
    discard_proxy_binary(&tmp.path().join("mockspace-proxy"), &mut actions);
    assert!(actions.is_empty(), "{actions:?}");
}
