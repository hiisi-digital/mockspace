//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A run answered from the store gives exactly what a full run gives, after every edit of a
//! history: an edit, a file added, a file removed, a file put back, a configuration changed.

use super::*;

fn severity(name: &str, s: Severity) -> LintConfig {
    let mut cfg = LintConfig::empty();
    cfg.base.insert(name.to_string(), s);
    cfg
}

type Step = (&'static str, fn(&Workspace));

#[test]
fn a_cached_run_equals_a_full_one_over_a_history_of_edits() {
    let w = Workspace::new("history", &[
        ("alpha", &[("src/lib.rs", "pub mod m;\n"), ("src/m.rs", "pub fn f() {}\n")]),
        ("bravo", &[("src/lib.rs", "//! docs\n\npub struct Thing;\n")]),
    ]);
    let cache = Mem::default();
    let off = severity("no-todo", Severity::OFF);
    let steps: &[Step] = &[
        ("start", |_| {}),
        ("nothing moved", |_| {}),
        ("a todo in a module", |w| w.write("alpha", "src/m.rs", "pub fn f() { todo!() }\n")),
        ("a module added", |w| w.write("alpha", "src/extra.rs", "pub fn g() { unimplemented!() }\n")),
        ("a crate root edited", |w| w.write("bravo", "src/lib.rs", "pub struct Thing;\npub struct Other;\n")),
        ("a module removed", |w| w.remove("alpha", "src/extra.rs")),
        ("the todo taken out", |w| w.write("alpha", "src/m.rs", "pub fn f() {}\n")),
        ("the todo put back", |w| w.write("alpha", "src/m.rs", "pub fn f() { todo!() }\n")),
        ("a crate root put back", |w| w.write("bravo", "src/lib.rs", "//! docs\n\npub struct Thing;\n")),
    ];
    for (what, step) in steps {
        step(&w);
        for cfg in [None, Some(&off)] {
            let full = shown(&w.run(None, cfg, None));
            let cached = shown(&w.run(Some(&cache), cfg, None));
            assert_eq!(cached, full, "after {what:?}, with configuration {:?}", cfg.map(|_| "no-todo off"));
        }
    }
    assert!(cache.remembered() > 0, "the control: the store was used at all");
}

#[test]
fn a_cached_run_replays_when_nothing_moved() {
    // The control on the test above: a store that never replayed would also equal the full run.
    let w = Workspace::new("history-replay", &[("alpha", &[("src/lib.rs", "pub fn f() { todo!() }\n")])]);
    let cache = Mem::default();
    let _ = w.run(Some(&cache), None, None);
    let reading = Reading::new(&w.dir.0);
    let again = shown(&w.run_with(&reading, Some(&cache), None, None));
    assert_eq!(again, shown(&w.run(None, None, None)));
    assert_eq!(reading.counts().parses, 0, "nothing was parsed, so nothing was judged");
}
