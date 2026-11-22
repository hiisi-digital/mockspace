//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Two views of one reading read, parse, list and start a command once between them.

use super::*;

fn ask_everything(v: &View<'_>) {
    assert_eq!(v.text("src/lib.rs"), Some("pub fn a() {}\n"));
    assert!(v.rust("src/lib.rs").is_some());
    assert!(v.toml("Cargo.toml").is_some_and(|t| t.is_ok()));
    assert_eq!(v.list("src").map(|l| l.entries().len()), Some(2));
    assert!(v.git(&["config", "--get", "core.bare"]).started());
    assert!(v.run("cat", &["src/other.rs"], v.root(), &[]).ok());
}

fn repo() -> Dir {
    let dir = Dir::new("sharing");
    dir.write("src/lib.rs", "pub fn a() {}\n")
        .write("src/other.rs", "fn b() {}\n")
        .write("Cargo.toml", "[package]\nname = \"x\"\n");
    dir.git(&["init", "-q"]);
    dir
}

#[test]
fn two_views_read_parse_list_and_spawn_once() {
    let dir = repo();
    let reading = Reading::new(&dir.0);
    ask_everything(&reading.view());
    let once = reading.counts();
    ask_everything(&reading.view());
    assert_eq!(reading.counts(), once, "the second view asked nothing of the disk");
    assert_eq!(
        (once.reads, once.parses, once.tomls, once.listings, once.spawns),
        (2, 1, 1, 1, 2),
        "each thing is produced once: {once:?}"
    );
}

#[test]
fn views_on_many_threads_still_produce_each_thing_once() {
    let dir = repo();
    let reading = Reading::new(&dir.0);
    std::thread::scope(|s| {
        for _ in 0 .. 8 {
            s.spawn(|| ask_everything(&reading.view()));
        }
    });
    let c = reading.counts();
    assert_eq!((c.reads, c.parses, c.tomls, c.listings, c.spawns), (2, 1, 1, 1, 2), "{c:?}");
}

#[test]
fn two_readings_share_nothing() {
    // The control: the counts above are the sharing and not a counter that never moves.
    let dir = repo();
    let a = Reading::new(&dir.0);
    let b = Reading::new(&dir.0);
    ask_everything(&a.view());
    ask_everything(&b.view());
    assert_eq!(a.counts(), b.counts());
    assert_eq!(a.counts().parses, 1);
}
