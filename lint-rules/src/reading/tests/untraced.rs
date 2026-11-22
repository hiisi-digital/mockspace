//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A unit marked `untraced` hands the mark to the items of its `each` calls, and an `each` asked of
//! the unit's view from inside an item saves nothing.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

fn one(message: String) -> Vec<LintError> {
    vec![LintError::error("workspace".into(), 1, "probe", message)]
}

fn joined(found: Vec<Vec<LintError>>) -> String {
    found.into_iter().flatten().map(|e| e.message).collect::<Vec<_>>().join(",")
}

/// What `side.txt` holds, read past the view as a lint marked `untraced` may.
fn side(v: &View<'_>) -> String {
    std::fs::read_to_string(v.reading().root().join("side.txt")).unwrap_or_default()
}

/// The unit says first that it reads past the view, then reads `side.txt` that way in its items.
fn untraced_then_each(v: &View<'_>) -> String {
    v.untraced("reads side.txt past the view");
    joined(v.each(&["a"], |iv, f| one(format!("{f}:{}+{}", iv.text(format!("{f}.txt")).unwrap_or("-"), side(iv)))))
}

#[test]
fn an_item_of_an_untraced_unit_is_judged_every_run() {
    let dir = Dir::new("untraced-items");
    dir.write("a.txt", "x").write("side.txt", "s");
    let cache = Mem::default();
    let (probe, _) = Probe::new(untraced_then_each);
    let pack = pack_of(probe);
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["a:x+s"]);
    dir.write("side.txt", "t");
    let fresh = messages(&run_repo(&dir, &Mem::default(), &pack, None).found);
    assert_eq!(fresh, ["a:x+t"], "the control: what a run with nothing saved finds");
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), fresh);
}

/// The unit is marked from inside an item, while the call's items are being judged.
fn untraced_inside_an_item(v: &View<'_>) -> String {
    joined(v.each(&["a"], |iv, f| {
        v.untraced("reads side.txt past the view");
        one(format!("{f}:{}+{}", iv.text(format!("{f}.txt")).unwrap_or("-"), side(iv)))
    }))
}

#[test]
fn a_mark_made_while_items_are_judged_saves_none_of_them() {
    let dir = Dir::new("untraced-during");
    dir.write("a.txt", "x").write("side.txt", "s");
    let cache = Mem::default();
    let (probe, _) = Probe::new(untraced_inside_an_item);
    let pack = pack_of(probe);
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["a:x+s"]);
    dir.write("side.txt", "t");
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["a:x+t"]);
}

static JUDGED_BEFORE: AtomicUsize = AtomicUsize::new(0);

/// An `each` ending before the unit is marked: its items read nothing past their own views.
fn each_then_untraced(v: &View<'_>) -> String {
    let out = joined(v.each(&["a"], |iv, f| {
        JUDGED_BEFORE.fetch_add(1, Ordering::SeqCst);
        one(format!("{f}:{}", iv.text(format!("{f}.txt")).unwrap_or("-")))
    }));
    v.untraced("reads the clock after its items");
    out
}

/// Items saved by a call that ended before the mark stay saved and replay, while the unit runs
/// every time.
#[test]
fn items_saved_before_the_mark_stay_saved() {
    let dir = Dir::new("untraced-after");
    dir.write("a.txt", "x");
    let cache = Mem::default();
    let (probe, judged) = Probe::new(each_then_untraced);
    let pack = pack_of(probe);
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["a:x"]);
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["a:x"]);
    assert_eq!(judged.load(Ordering::SeqCst), 2, "the unit ran both times");
    assert_eq!(JUDGED_BEFORE.load(Ordering::SeqCst), 1, "its item was replayed the second time");
    dir.write("a.txt", "y");
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["a:y"], "and judged again when it moved");
}

/// Outer items in the order `order` names, each asking the unit's own view for an `each` whose
/// item reads the file of the outer item that asked it.
fn nested_on_the_outer_view(v: &View<'_>) -> String {
    let order: Vec<String> = v.text("order").unwrap_or_default().split(',').map(str::to_string).collect();
    joined(v.each(&order, |_, outer| {
        v.each(&["z"], |iv, _| one(format!("{outer}:{}", iv.text(format!("{outer}.txt")).unwrap_or("-")))).concat()
    }))
}

/// An `each` asked of the unit's view from inside an item is a stray question to that view, so it
/// saves none of its items, which are keyed by a count that follows whichever outer item asked
/// first.
#[test]
fn an_each_on_the_outer_view_from_an_item_saves_nothing() {
    let dir = Dir::new("stray-nested");
    dir.write("a.txt", "1").write("b.txt", "2").write("order", "a,b");
    let cache = Mem::default();
    let (probe, _) = Probe::new(nested_on_the_outer_view);
    let pack = pack_of(probe);
    let one_thread = || Reading::new(&dir.0).with_threads(1);
    assert_eq!(messages(&run_repo_over(one_thread(), &dir, &cache, &pack, None).found), ["a:1,b:2"]);
    dir.write("order", "b,a");
    let fresh = messages(&run_repo_over(one_thread(), &dir, &Mem::default(), &pack, None).found);
    assert_eq!(fresh, ["b:2,a:1"], "the control: what a run with nothing saved finds");
    assert_eq!(messages(&run_repo_over(one_thread(), &dir, &cache, &pack, None).found), fresh);
}
